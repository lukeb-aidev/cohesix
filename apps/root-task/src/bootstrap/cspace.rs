// Author: Lukas Bower
#![allow(unsafe_code)]

#[cfg(feature = "cap-probes")]
use crate::bootstrap::log::force_uart_line;
#[cfg(feature = "cap-probes")]
use crate::bootstrap::retype::{bump_slot, retype_captable};
use crate::cspace::{cap_rights_read_write_grant, CSpace};
use crate::sel4::{
    self, canonical_root_cap_ptr, cap_data_guard, is_boot_reserved_slot,
    publish_canonical_root_alias, BootInfoExt, BootInfoView, WORD_BITS,
};
#[cfg(feature = "cap-probes")]
use core::convert::TryFrom;
use core::fmt::Write;
use heapless::String;

use super::cspace_sys;
use sel4_sys::{
    self, seL4_BootInfo, seL4_CNode, seL4_CPtr, seL4_CapBootInfoFrame, seL4_CapInitThreadCNode,
    seL4_CapInitThreadTCB, seL4_CapRights_All, seL4_NoError, seL4_Word,
};

#[cfg(feature = "cap-probes")]
use sel4_sys::seL4_Error;

const MAX_DIAGNOSTIC_LEN: usize = 224;

#[derive(Copy, Clone, Debug)]
/// Canonical projection of the init thread CSpace window captured from bootinfo.
pub struct CSpaceWindow {
    /// Root CNode capability designating the init thread's CSpace.
    pub root: sel4::seL4_CPtr,
    /// Canonical guard-less root capability that can address kernel slots below the advertised window.
    pub canonical_root: sel4::seL4_CPtr,
    /// Radix width (in bits) of the init CNode as reported by bootinfo.
    pub bits: u8,
    /// First free slot index inside the init CNode window.
    pub first_free: sel4::seL4_CPtr,
    /// Lower bound (inclusive) of the init CSpace free window advertised by the kernel.
    pub empty_start: sel4::seL4_CPtr,
    /// Upper bound (exclusive) of the init CSpace free window advertised by the kernel.
    pub empty_end: sel4::seL4_CPtr,
}

/// Ensures a canonical alias for the init root CNode exists and tracks it.
pub fn ensure_canonical_root_alias(
    bi: &seL4_BootInfo,
) -> Result<sel4::seL4_CPtr, sel4::seL4_Error> {
    let current = sel4::canonical_root_cap_ptr();
    if current != seL4_CapInitThreadCNode {
        ::log::info!("[cnode] canonical alias already installed alias=0x{current:04x}");
        return Ok(current);
    }

    let empty_start = bi.empty_first_slot() as sel4::seL4_CPtr;
    let empty_end = bi.empty_last_slot_excl() as sel4::seL4_CPtr;
    assert!(
        empty_start < empty_end,
        "bootinfo empty window must not be empty"
    );
    let alias_slot = empty_end
        .checked_sub(1)
        .expect("bootinfo empty window must contain at least one slot");
    assert!(
        alias_slot >= empty_start,
        "alias slot fell outside the bootinfo empty window"
    );
    assert!(
        !is_boot_reserved_slot(alias_slot),
        "alias slot collides with a kernel-reserved capability"
    );

    let init_bits = bi.initThreadCNodeSizeBits as u8;
    let guard_size = sel4::word_bits()
        .checked_sub(init_bits as sel4::seL4_Word)
        .expect("word bits must exceed init cnode bits");
    let cap_data = cap_data_guard(0, guard_size);
    let rights = sel4::SeL4CapRights::new(1, 1, 1, 1);

    ::log::info!(
        "[cnode] mint canonical alias slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    let depth = sel4::word_bits() as u8;
    let dst_index = cspace_sys::enc_index(
        alias_slot as seL4_Word,
        bi,
        cspace_sys::TupleStyle::GuardEncoded,
    ) as sel4::seL4_CPtr;
    let src_index = seL4_CapInitThreadCNode as seL4::seL4_Word;
    let err = sel4::cnode_mint_depth(
        seL4_CapInitThreadCNode,
        dst_index,
        depth,
        seL4_CapInitThreadCNode,
        src_index,
        init_bits,
        rights,
        cap_data,
    );
    if err != seL4_NoError {
        ::log::error!(
            "[cnode] canonical alias mint failed slot=0x{alias_slot:04x} err={err} ({name})",
            name = sel4::error_name(err),
        );
        return Err(err);
    }

    publish_canonical_root_alias(alias_slot);
    ::log::info!(
        "[cnode] canonical alias ready slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    Ok(alias_slot)
}

fn locate_bootinfo_frame_slot(bi: &seL4_BootInfo) -> Option<sel4::seL4_CPtr> {
    let canonical = seL4_CapBootInfoFrame as sel4::seL4_CPtr;
    if sel4::debug_cap_identify(canonical) != 0 {
        return Some(canonical);
    }
    let (start, end) = bi.extra_bipage_slots();
    if start >= end {
        return None;
    }
    for slot in start as sel4::seL4_CPtr..end as sel4::seL4_CPtr {
        if sel4::debug_cap_identify(slot) != 0 {
            return Some(slot);
        }
    }
    None
}

impl CSpaceWindow {
    /// Constructs a canonical window from the supplied bootinfo view.
    #[must_use]
    pub fn from_bootinfo(view: &BootInfoView) -> Self {
        let (first_free, end) = view.init_cnode_empty_range();
        Self {
            root: view.root_cnode_cap(),
            canonical_root: view.canonical_root_cap(),
            bits: cspace_sys::bits_as_u8(usize::from(view.init_cnode_bits())),
            first_free,
            empty_start: first_free,
            empty_end: end,
        }
    }

    /// Advances the first-free slot pointer, avoiding slot reuse between placements.
    pub fn bump(&mut self) {
        self.first_free = self.first_free.wrapping_add(1);
        debug_assert!(
            self.first_free <= self.empty_end,
            "first_free advanced beyond bootinfo window end"
        );
    }

    /// Returns `true` when `slot` lies within the bootinfo-advertised free window.
    #[must_use]
    pub fn contains(&self, slot: sel4::seL4_CPtr) -> bool {
        slot >= self.empty_start && slot < self.empty_end
    }

    /// Asserts that `slot` lies within the bootinfo-advertised free window.
    pub fn assert_contains(&self, slot: sel4::seL4_CPtr) {
        assert!(
            self.contains(slot),
            "slot 0x{slot:04x} outside bootinfo window [0x{start:04x}..0x{end:04x})",
            slot = slot,
            start = self.empty_start,
            end = self.empty_end,
        );
    }
}

/// Ensures the init CNode exposes a guard-less alias for canonical tuples.
pub fn ensure_canonical_root_alias(
    bi: &seL4_BootInfo,
) -> Result<sel4::seL4_CPtr, sel4::seL4_Error> {
    let current = canonical_root_cap_ptr();
    if current != seL4_CapInitThreadCNode {
        ::log::info!(
            "[cnode] canonical alias already installed alias=0x{current:04x}",
            current = current
        );
        return Ok(current);
    }

    let empty_start = bi.empty_first_slot() as sel4::seL4_CPtr;
    let empty_end = bi.empty_last_slot_excl() as sel4::seL4_CPtr;
    assert!(
        empty_start < empty_end,
        "bootinfo empty window must not be empty"
    );
    let alias_slot = empty_end
        .checked_sub(1)
        .expect("bootinfo empty window must contain at least one slot");
    assert!(
        alias_slot >= empty_start,
        "alias slot fell outside the bootinfo empty window"
    );
    assert!(
        !is_boot_reserved_slot(alias_slot),
        "alias slot collides with a kernel-reserved capability"
    );

    let init_bits = bi.initThreadCNodeSizeBits as u8;
    let guard_size = sel4::word_bits()
        .checked_sub(init_bits as sel4::seL4_Word)
        .expect("word bits must exceed init cnode bits");
    let cap_data = cap_data_guard(0, guard_size);
    let rights = sel4::SeL4CapRights::new(1, 1, 1, 1);

    ::log::info!(
        "[cnode] mint canonical alias slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    let depth = sel4::word_bits() as u8;
    let dst_index = cspace_sys::enc_index(
        alias_slot as seL4_Word,
        bi,
        cspace_sys::TupleStyle::GuardEncoded,
    ) as sel4::seL4_CPtr;
    let src_index = seL4_CapInitThreadCNode as seL4::seL4_Word;
    let err = sel4::cnode_mint_depth(
        seL4_CapInitThreadCNode,
        dst_index,
        depth,
        seL4_CapInitThreadCNode,
        src_index,
        init_bits,
        rights,
        cap_data,
    );
    if err != seL4_NoError {
        ::log::error!(
            "[cnode] canonical alias mint failed slot=0x{alias_slot:04x} err={err} ({name})",
            name = sel4::error_name(err),
        );
        return Err(err);
    }

    publish_canonical_root_alias(alias_slot);
    ::log::info!(
        "[cnode] canonical alias ready slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    Ok(alias_slot)
}

/// Mints and caches a canonical alias for the init CNode if not already created.
pub fn ensure_canonical_root_alias(
    bi: &seL4_BootInfo,
) -> Result<sel4::seL4_CPtr, sel4::seL4_Error> {
    let current = canonical_root_cap_ptr();
    if current != seL4_CapInitThreadCNode {
        ::log::info!("[cnode] canonical alias already installed alias=0x{current:04x}");
        return Ok(current);
    }

    let empty_start = bi.empty_first_slot() as sel4::seL4_CPtr;
    let empty_end = bi.empty_last_slot_excl() as sel4::seL4_CPtr;
    assert!(
        empty_start < empty_end,
        "bootinfo empty window must not be empty"
    );
    let alias_slot = empty_end
        .checked_sub(1)
        .expect("bootinfo empty window must contain at least one slot");
    assert!(
        alias_slot >= empty_start,
        "alias slot fell outside the bootinfo empty window"
    );
    assert!(
        !is_boot_reserved_slot(alias_slot),
        "alias slot collides with a kernel-reserved capability"
    );

    let init_bits = bi.initThreadCNodeSizeBits as u8;
    let guard_size = sel4::word_bits()
        .checked_sub(init_bits as sel4::seL4_Word)
        .expect("word bits must exceed init cnode bits");
    let cap_data = cap_data_guard(0, guard_size);
    let rights = sel4::SeL4CapRights::new(1, 1, 1, 1);

    ::log::info!(
        "[cnode] mint canonical alias slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    let depth = sel4::word_bits() as u8;
    let dst_index = cspace_sys::enc_index(
        alias_slot as seL4_Word,
        bi,
        cspace_sys::TupleStyle::GuardEncoded,
    ) as sel4::seL4_CPtr;
    let src_index = seL4_CapInitThreadCNode as seL4::seL4_Word;
    let err = sel4::cnode_mint_depth(
        seL4_CapInitThreadCNode,
        dst_index,
        depth,
        seL4_CapInitThreadCNode,
        src_index,
        init_bits,
        rights,
        cap_data,
    );
    if err != seL4_NoError {
        ::log::error!(
            "[cnode] canonical alias mint failed slot=0x{alias_slot:04x} err={err} ({name})",
            name = sel4::error_name(err),
        );
        return Err(err);
    }

    publish_canonical_root_alias(alias_slot);
    ::log::info!(
        "[cnode] canonical alias ready slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    Ok(alias_slot)
}

/// Ensures the init CSpace exposes a canonical alias that handles guard depth properly.
pub fn ensure_canonical_root_alias(
    bi: &seL4_BootInfo,
) -> Result<sel4::seL4_CPtr, sel4::seL4_Error> {
    let current = canonical_root_cap_ptr();
    if current != seL4_CapInitThreadCNode {
        ::log::info!("[cnode] canonical alias already installed alias=0x{current:04x}");
        return Ok(current);
    }

    let empty_start = bi.empty_first_slot() as sel4::seL4_CPtr;
    let empty_end = bi.empty_last_slot_excl() as sel4::seL4_CPtr;
    assert!(
        empty_start < empty_end,
        "bootinfo empty window must not be empty"
    );
    let alias_slot = empty_end
        .checked_sub(1)
        .expect("bootinfo empty window must contain at least one slot");
    assert!(
        alias_slot >= empty_start,
        "alias slot fell outside the bootinfo empty window"
    );
    assert!(
        !is_boot_reserved_slot(alias_slot),
        "alias slot collides with a kernel-reserved capability"
    );

    let init_bits = bi.initThreadCNodeSizeBits as u8;
    let guard_size = sel4::word_bits()
        .checked_sub(init_bits as sel4::seL4_Word)
        .expect("word bits must exceed init cnode bits");
    let cap_data = cap_data_guard(0, guard_size);
    let rights = sel4::SeL4CapRights::new(1, 1, 1, 1);

    ::log::info!(
        "[cnode] mint canonical alias slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    let depth = sel4::word_bits() as u8;
    let dst_index = cspace_sys::enc_index(
        alias_slot as seL4_Word,
        bi,
        cspace_sys::TupleStyle::GuardEncoded,
    ) as sel4::seL4_CPtr;
    let src_index = seL4_CapInitThreadCNode as sel4::seL4_Word;
    let err = sel4::cnode_mint_depth(
        seL4_CapInitThreadCNode,
        dst_index,
        depth,
        seL4_CapInitThreadCNode,
        src_index,
        init_bits,
        rights,
        cap_data,
    );
    if err != seL4_NoError {
        ::log::error!(
            "[cnode] canonical alias mint failed slot=0x{alias_slot:04x} err={err} ({name})",
            name = sel4::error_name(err),
        );
        return Err(err);
    }

    publish_canonical_root_alias(alias_slot);
    ::log::info!(
        "[cnode] canonical alias ready slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    Ok(alias_slot)
}

/// Ensures the init CSpace exposes a guard-encoded alias that accepts canonical tuples.
pub fn ensure_canonical_root_alias(
    bi: &seL4_BootInfo,
) -> Result<sel4::seL4_CPtr, sel4::seL4_Error> {
    let current = sel4::canonical_root_cap_ptr();
    if current != seL4_CapInitThreadCNode {
        ::log::info!(
            "[cnode] canonical alias already installed alias=0x{current:04x}",
            current = current
        );
        return Ok(current);
    }

    let empty_start = bi.empty_first_slot() as sel4::seL4_CPtr;
    let empty_end = bi.empty_last_slot_excl() as sel4::seL4_CPtr;
    assert!(
        empty_start < empty_end,
        "bootinfo empty window must not be empty"
    );
    let alias_slot = empty_end
        .checked_sub(1)
        .expect("bootinfo empty window must contain at least one slot");
    assert!(
        alias_slot >= empty_start,
        "alias slot fell outside the bootinfo empty window"
    );
    assert!(
        !is_boot_reserved_slot(alias_slot),
        "alias slot collides with a kernel-reserved capability"
    );

    let init_bits = bi.initThreadCNodeSizeBits as u8;
    let guard_size = sel4::word_bits()
        .checked_sub(init_bits as sel4::seL4_Word)
        .expect("word bits must exceed init cnode bits");
    let cap_data = cap_data_guard(0, guard_size);
    let rights = sel4::SeL4CapRights::new(1, 1, 1, 1);
    ::log::info!(
        "[cnode] mint canonical alias slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    let style = cspace_sys::TupleStyle::GuardEncoded;
    let dst_index = cspace_sys::enc_index(alias_slot as seL4_Word, bi, style) as sel4::seL4_CPtr;
    let dst_depth = sel4::word_bits() as u8;
    let src_index = seL4_CapInitThreadCNode as sel4::seL4_Word;
    let src_depth = init_bits;
    let err = sel4::cnode_mint_depth(
        seL4_CapInitThreadCNode,
        dst_index,
        dst_depth,
        seL4_CapInitThreadCNode,
        src_index,
        src_depth,
        rights,
        cap_data,
    );
    if err != seL4_NoError {
        ::log::error!(
            "[cnode] canonical alias mint failed slot=0x{alias_slot:04x} err={err} ({name})",
            name = sel4::error_name(err),
        );
        return Err(err);
    }
    publish_canonical_root_alias(alias_slot);
    ::log::info!(
        "[cnode] canonical alias ready slot=0x{alias_slot:04x} guard_bits={guard_size}",
        guard_size = guard_size
    );
    Ok(alias_slot)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Canonical representation of a capability path targeting a CNode.
pub struct CNodePath {
    /// Root capability selecting the destination CSpace.
    pub root: seL4_CPtr,
    /// Capability pointer identifying the destination CNode object.
    pub index: seL4_CPtr,
    /// Guard depth (in bits) associated with the destination CNode pointer.
    pub depth: seL4_Word,
}

impl CNodePath {
    /// Construct a new path descriptor.
    #[must_use]
    pub const fn new(root: seL4_CPtr, index: seL4_CPtr, depth: seL4_Word) -> Self {
        Self { root, index, depth }
    }

    /// Render the `(root, index, depth, offset)` tuple expected by seL4 syscalls.
    #[must_use]
    pub const fn as_tuple(
        &self,
        offset: seL4_Word,
    ) -> (seL4_CPtr, seL4_Word, seL4_Word, seL4_Word) {
        (self.root, self.index as seL4_Word, self.depth, offset)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Half-open slot interval within a CNode.
pub struct SlotRange {
    /// First usable slot within the range.
    pub start: seL4_Word,
    /// Exclusive end of the slot interval.
    pub end: seL4_Word,
}

impl SlotRange {
    /// Construct a range and validate ordering.
    #[must_use]
    pub const fn new(start: seL4_Word, end: seL4_Word) -> Self {
        Self { start, end }
    }

    /// Returns `true` when `slot` lies within the interval.
    #[must_use]
    pub const fn contains(&self, slot: seL4_Word) -> bool {
        slot >= self.start && slot < self.end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Canonical view of the init thread's root CNode as advertised by bootinfo.
pub struct InitCNode {
    /// Fully-qualified capability path to the init CNode.
    pub path: CNodePath,
    /// Empty slot interval reserved for the root task.
    pub empty: SlotRange,
    /// Radix width (in bits) of the init CNode object.
    pub bits: u8,
}

impl InitCNode {
    /// Build the descriptor from kernel boot information.
    #[must_use]
    pub fn from_bootinfo(bi: &seL4_BootInfo) -> Self {
        let root = seL4_CapInitThreadCNode;
        let depth = bi.initThreadCNodeSizeBits as seL4_Word;
        let index = 0;
        let bits = bi.initThreadCNodeSizeBits as u8;
        let empty = SlotRange::new(bi.empty.start as seL4_Word, bi.empty.end as seL4_Word);
        Self {
            path: CNodePath::new(root, index, depth),
            empty,
            bits,
        }
    }

    /// Assert that `slot` resides within the bootinfo-provided empty window.
    pub fn assert_slot(&self, slot: seL4_Word) {
        assert!(
            self.empty.contains(slot),
            "slot 0x{slot:04x} outside init empty window [0x{start:04x}..0x{end:04x})",
            start = self.empty.start,
            end = self.empty.end,
        );
    }
}

#[inline(always)]
fn root_cnode() -> seL4_CNode {
    seL4_CapInitThreadCNode
}

#[inline(always)]
fn idx(slot: usize) -> seL4_CPtr {
    slot as seL4_CPtr
}

fn assert_caps_known() {
    debug_assert_eq!(seL4_CapInitThreadCNode as usize, 2);
    debug_assert_eq!(seL4_CapBootInfoFrame as usize, 9);
    debug_assert_eq!(seL4_CapInitThreadTCB as usize, 1);
}

fn cap_type_of(slot: usize) -> u32 {
    unsafe { sel4_sys::seL4_DebugCapIdentify(idx(slot)) }
}

#[inline(always)]
pub fn root_cnode_path(
    init_cnode_bits: u8,
    dst_slot: seL4_Word,
) -> (seL4_CPtr, seL4_Word, seL4_Word, seL4_Word) {
    let depth = usize::from(init_cnode_bits) as seL4_Word;
    let path = CNodePath::new(root_cnode(), 0, depth);
    guard_root_path(
        init_cnode_bits,
        path.index as seL4_Word,
        path.depth,
        dst_slot,
    );
    path.as_tuple(dst_slot)
}

#[inline(always)]
pub fn guard_root_path(init_cnode_bits: u8, index: seL4_Word, depth: seL4_Word, offset: seL4_Word) {
    let expected_depth = usize::from(init_cnode_bits) as seL4_Word;
    assert_eq!(depth, expected_depth, "depth must equal init cnode bits");
    assert_eq!(
        index, 0,
        "node index must be zero for init CNode direct path"
    );
    let limit = if init_cnode_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_cnode_bits
    };
    assert!((offset as usize) < limit, "slot out of range",);
}

#[inline(always)]
pub fn assert_root_path(
    init_cnode_bits: u8,
    index: seL4_Word,
    depth: seL4_Word,
    offset: seL4_Word,
) {
    guard_root_path(init_cnode_bits, index, depth, offset);
}

fn log_boot(beg: sel4::seL4_CPtr, end: sel4::seL4_CPtr, bits: u8) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    if write!(
        &mut line,
        "[boot] empty=[{:#x}..{:#x}) cnode_bits={}",
        beg, end, bits
    )
    .is_err()
    {
        // Truncated diagnostic; best effort only.
    }
    emit_console_line(line.as_str());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Structurally typed destination descriptor for seL4 retype operations.
pub struct DestCNode {
    /// Capability to the root CNode of the destination CSpace.
    pub root: sel4::seL4_CPtr,
    /// Radix width (in bits) of the destination CNode.
    pub root_bits: u8,
    /// First free slot index advertised by bootinfo.
    pub empty_start: u32,
    /// One-past-the-end bound of the bootinfo empty window.
    pub empty_end: u32,
    /// Current insertion slot within the root CNode.
    pub slot_offset: u32,
}

impl DestCNode {
    #[inline(always)]
    fn cap_slots(&self) -> u32 {
        1u32 << self.root_bits
    }

    /// Verifies that the destination invariants remain sane.
    pub fn assert_sane(&self) {
        assert!(
            self.root_bits <= 31,
            "root_bits must be <= 31 (got {})",
            self.root_bits
        );
        assert!(
            self.empty_start < self.empty_end,
            "empty window must be non-empty"
        );
        let cap_slots = self.cap_slots();
        assert!(
            self.empty_end <= cap_slots,
            "empty window end 0x{end:04x} exceeds cnode capacity 0x{cap:04x}",
            end = self.empty_end,
            cap = cap_slots,
        );
        assert!(
            self.slot_offset >= self.empty_start && self.slot_offset < self.empty_end,
            "slot 0x{slot:04x} outside empty window [0x{start:04x}..0x{end:04x})",
            slot = self.slot_offset,
            start = self.empty_start,
            end = self.empty_end,
        );
    }

    #[inline(always)]
    pub fn path_label(&self) -> &'static str {
        "direct:init-cnode"
    }

    #[inline(always)]
    fn validate_slot(&self, slot: u32) {
        assert!(
            slot >= self.empty_start && slot < self.empty_end,
            "slot 0x{slot:04x} outside empty window [0x{start:04x}..0x{end:04x})",
            slot = slot,
            start = self.empty_start,
            end = self.empty_end,
        );
        let cap_slots = self.cap_slots();
        assert!(
            slot < cap_slots,
            "slot 0x{slot:04x} exceeds cnode capacity 0x{cap:04x}",
            slot = slot,
            cap = cap_slots,
        );
    }

    #[inline(always)]
    pub fn set_slot_offset(&mut self, slot: sel4::seL4_CPtr) {
        let slot_u32 = slot
            .try_into()
            .expect("slot offset must fit within u32 for init CNode");
        self.validate_slot(slot_u32);
        self.slot_offset = slot_u32;
    }

    #[inline(always)]
    pub fn bump_slot(&mut self) {
        self.slot_offset = self
            .slot_offset
            .checked_add(1)
            .expect("slot offset overflow");
        assert!(self.slot_offset <= self.empty_end, "ran out of empty slots",);
    }
}

/// Constructs a destination descriptor anchored at the root CNode.
pub fn make_root_dest(bi: &sel4_sys::seL4_BootInfo) -> DestCNode {
    let init = InitCNode::from_bootinfo(bi);
    let root = init.path.root;
    let depth_bits = init.bits;
    let empty_start: u32 = init
        .empty
        .start
        .try_into()
        .expect("empty window start must fit within u32");
    let empty_end: u32 = init
        .empty
        .end
        .try_into()
        .expect("empty window end must fit within u32");
    let dest = DestCNode {
        root,
        root_bits: depth_bits,
        empty_start,
        empty_end,
        slot_offset: empty_start,
    };
    dest.assert_sane();
    dest
}

pub fn prove_dest_path_with_bootinfo(
    bi: &sel4_sys::seL4_BootInfo,
    first_free: sel4::seL4_CPtr,
) -> Result<(), sel4_sys::seL4_Error> {
    assert_caps_known();
    let dst_slot_raw = first_free as usize;
    let dst_slot_word = dst_slot_raw as seL4_Word;
    let Some(source_slot) = locate_bootinfo_frame_slot(bi) else {
        ::log::info!("[probe] BootInfo frame capability not present — skipping slot verification");
        return Ok(());
    };
    let rights = seL4_CapRights_All;
    let dst_root = root_cnode();

    cspace_sys::debug_identify_cap("InitCNode", dst_root as seL4_CPtr);
    cspace_sys::assert_init_cnode_layout(bi);

    let style = cspace_sys::tuple_style();
    let _ = cspace_sys::cnode_delete_with_style(bi, dst_root as seL4_CNode, dst_slot_word, style);

    let err = cspace_sys::canonical_cnode_copy(bi, dst_slot_word, source_slot as seL4_Word, rights);
    ::log::info!(
        "[probe] copy BootInfo frame slot=0x{source_slot:04x} -> 0x{dst_slot_raw:04x} err={err}",
        source_slot = source_slot,
        dst_slot_raw = dst_slot_raw,
        err = err
    );

    let _ = cspace_sys::cnode_delete_with_style(bi, dst_root as seL4_CNode, dst_slot_word, style);

    if err == seL4_NoError {
        return Ok(());
    }

    ::log::warn!(
        "[probe] BootInfo frame copy failed err={} — continuing without slot verification",
        err
    );
    Ok(())
}

#[cfg(feature = "cap-probes")]
pub fn cnode_copy_selftest(bi: &seL4_BootInfo) -> Result<(), seL4_Error> {
    let (start, _end) = crate::sel4_view::empty_window(bi);
    let root = root_cnode();

    let dst_slot = idx(start as usize) as seL4_Word;
    let src_slot = idx(seL4_CapInitThreadCNode as usize) as seL4_Word;
    let rights = seL4_CapRights_All;
    log::info!(
        "[selftest] copy init CNode cap into slot 0x{start:04x} (root=0x{root:04x})",
        root = root,
    );

    let canonical_root = bi.canonical_root_cap();
    let copy_err = cspace_sys::cnode_copy(bi, root, dst_slot, canonical_root, src_slot, rights);

    if copy_err != seL4_NoError {
        log::warn!("[selftest] CNode_Copy failed err={copy_err}");
        return Err(copy_err);
    }

    log::info!("[selftest] CNode_Copy OK (slot {start:#04x})");

    #[cfg(target_os = "none")]
    {
        let depth_bits = sel4::init_cnode_depth(bi);
        let delete_err = unsafe { seL4_CNode_Delete(root, idx(start as usize), depth_bits) };
        if delete_err != seL4_NoError {
            log::warn!("[selftest] cleanup delete failed err={delete_err}");
            return Err(delete_err);
        }
    }

    Ok(())
}

#[cfg(feature = "canonical_cspace")]
pub fn first_endpoint_retype(
    bi: &seL4_BootInfo,
    ut_cap: seL4_CPtr,
    slot: seL4_CPtr,
) -> Result<(), seL4_Error> {
    let dst_root = root_cnode();
    let node_index = sel4::init_cnode_index_word() as u32;
    let node_depth = sel4::init_cnode_depth(bi) as seL4_Word;
    let node_off = slot as seL4_Word;

    log::info!(
        "[retype] request ut=0x{ut_cap:04x} root=0x{dst_root:04x} idx={node_index} depth={node_depth} off=0x{node_off:04x}",
    );

    cspace_sys::retype_into_root(
        ut_cap,
        sel4_sys::seL4_ObjectType::seL4_EndpointObject as _,
        0,
        slot,
        bi,
    )
}

/// Performs the initial proof-of-life retype calls against the init CNode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstRetypeResult {
    /// Slot containing a copied init TCB capability used for sanity checks.
    pub tcb_copy_slot: sel4::seL4_CPtr,
    /// Slot populated with the freshly retyped endpoint.
    pub endpoint_slot: sel4::seL4_CPtr,
    /// Slot populated with the scratch CNode used for later allocations.
    pub captable_slot: sel4::seL4_CPtr,
}

#[cfg(all(feature = "cap-probes", not(feature = "canonical_cspace")))]
pub fn cspace_first_retypes(
    bi: &sel4_sys::seL4_BootInfo,
    cs: &mut CSpace,
    ut_cap: sel4_sys::seL4_CPtr,
) -> Result<FirstRetypeResult, sel4_sys::seL4_Error> {
    let init = InitCNode::from_bootinfo(bi);
    let mut dest = make_root_dest(bi);
    let mut init_line = String::<MAX_DIAGNOSTIC_LEN>::new();
    if write!(
        &mut init_line,
        "[cspace:init] root={:#06x} bits={} window=[{:#06x}..{:#06x})",
        init.path.root, init.bits, init.empty.start, init.empty.end,
    )
    .is_err()
    {
        // Partial diagnostics are acceptable.
    }
    force_uart_line(init_line.as_str());
    dest.set_slot_offset(cs.next_free_slot());
    dest.assert_sane();

    let tcb_copy_slot = match cs.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(&mut line, "[cnode:copy] slot alloc err={}", err as i32).is_err() {}
            force_uart_line(line.as_str());
            return Err(err);
        }
    };
    dest.set_slot_offset(tcb_copy_slot);
    let copy_err = cspace_sys::cnode_copy_raw_single(
        bi,
        seL4_CapInitThreadCNode as seL4_CNode,
        tcb_copy_slot as seL4_Word,
        seL4_CapInitThreadCNode as seL4_CNode,
        seL4_CapInitThreadTCB as seL4_Word,
    );
    let mut copy_line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let depth_word = sel4::init_cnode_depth(bi) as seL4_Word;
    let src_slot = seL4_CapInitThreadTCB as seL4_Word;
    let log_result = if copy_err == sel4_sys::seL4_NoError {
        write!(
            &mut copy_line,
            "[cnode:copy] src=seL4_CapInitThreadTCB slot=0x{src_slot:04x} src_depth={depth} \
             -> dst=0x{dst_slot:04x} dst_depth={dst_depth} OK",
            src_slot = src_slot,
            depth = depth_word,
            dst_slot = tcb_copy_slot,
            dst_depth = depth_word,
        )
    } else {
        write!(
            &mut copy_line,
            "[cnode:copy] src=seL4_CapInitThreadTCB slot=0x{src_slot:04x} src_depth={depth} \
             -> dst=0x{dst_slot:04x} dst_depth={dst_depth} ERR={err}",
            src_slot = src_slot,
            depth = depth_word,
            dst_slot = tcb_copy_slot,
            dst_depth = depth_word,
            err = copy_err as i32,
        )
    };
    if log_result.is_err() {
        // Partial diagnostics are acceptable.
    }
    force_uart_line(copy_line.as_str());
    if copy_err != sel4_sys::seL4_NoError {
        return Err(copy_err);
    }
    bump_slot(&mut dest);

    let endpoint_slot = match cs.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(
                &mut line,
                "[cspace:init] endpoint slot alloc err={}",
                err as i32,
            )
            .is_err()
            {}
            force_uart_line(line.as_str());
            return Err(err);
        }
    };
    dest.set_slot_offset(endpoint_slot);
    cspace_sys::retype_into_root(
        ut_cap,
        sel4_sys::seL4_ObjectType::seL4_EndpointObject as _,
        0,
        endpoint_slot,
        bi,
    )?;
    bump_slot(&mut dest);

    let captable_slot = match cs.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(
                &mut line,
                "[cspace:init] captable slot alloc err={}",
                err as i32,
            )
            .is_err()
            {}
            force_uart_line(line.as_str());
            return Err(err);
        }
    };
    dest.set_slot_offset(captable_slot);
    let captable_err = retype_captable(ut_cap as sel4_sys::seL4_Word, 4, &dest);
    if captable_err != sel4_sys::seL4_NoError {
        return Err(captable_err);
    }
    bump_slot(&mut dest);

    Ok(FirstRetypeResult {
        tcb_copy_slot,
        endpoint_slot,
        captable_slot,
    })
}

#[cfg(all(feature = "cap-probes", feature = "canonical_cspace"))]
pub fn cspace_first_retypes(
    bi: &sel4_sys::seL4_BootInfo,
    cs: &mut CSpace,
    ut_cap: sel4_sys::seL4_CPtr,
) -> Result<FirstRetypeResult, sel4_sys::seL4_Error> {
    let init = InitCNode::from_bootinfo(bi);
    let mut dest = make_root_dest(bi);
    let mut init_line = String::<MAX_DIAGNOSTIC_LEN>::new();
    if write!(
        &mut init_line,
        "[cspace:init] root={:#06x} bits={} window=[{:#06x}..{:#06x})",
        init.path.root, init.bits, init.empty.start, init.empty.end,
    )
    .is_err()
    {
        // Partial diagnostics are acceptable.
    }
    force_uart_line(init_line.as_str());
    dest.set_slot_offset(cs.next_free_slot());
    dest.assert_sane();

    cnode_copy_selftest(bi).expect("[selftest] cnode.copy failed");

    let rights = cap_rights_read_write_grant();

    let endpoint_slot = match cs.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(
                &mut line,
                "[cspace:init] endpoint slot alloc err={}",
                err as i32
            )
            .is_err()
            {}
            force_uart_line(line.as_str());
            return Err(err);
        }
    };

    dest.set_slot_offset(endpoint_slot);
    dest.assert_sane();

    let endpoint_slot_u32 = u32::try_from(endpoint_slot)
        .expect("endpoint slot must fit within 32 bits for canonical operations");

    let (window_start, _) = crate::sel4_view::empty_window(bi);
    let window_start_u32 = u32::try_from(window_start)
        .expect("bootinfo empty window start must fit within u32 for canonical operations");
    debug_assert_eq!(
        endpoint_slot_u32, window_start_u32,
        "first canonical endpoint slot should align with bootinfo window",
    );

    first_endpoint_retype(bi, ut_cap, endpoint_slot)
        .map_err(|e| panic!("[retype] endpoint fail slot=0x{endpoint_slot_u32:04x} err={e:?}"))?;

    log::info!("[retype:ok] endpoint @ slot=0x{:04x}", endpoint_slot_u32);
    #[cfg(feature = "canonical_cspace")]
    {
        crate::console::start(endpoint_slot_u32, bi);
    }

    bump_slot(&mut dest);

    let tcb_copy_slot = match cs.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(&mut line, "[cnode:copy] slot alloc err={}", err as i32).is_err() {}
            force_uart_line(line.as_str());
            return Err(err);
        }
    };
    dest.set_slot_offset(tcb_copy_slot);
    let copy_err = cspace_sys::cnode_copy(
        bi,
        seL4_CapInitThreadCNode,
        tcb_copy_slot as seL4_Word,
        seL4_CapInitThreadCNode,
        seL4_CapInitThreadTCB as seL4_Word,
        rights,
    );
    let mut copy_line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let depth_word = sel4::init_cnode_depth(bi) as seL4_Word;
    let src_slot = seL4_CapInitThreadTCB as seL4_Word;
    let log_result = if copy_err == sel4_sys::seL4_NoError {
        write!(
            &mut copy_line,
            "[cnode:copy] src=seL4_CapInitThreadTCB slot=0x{src_slot:04x} src_depth={depth} \
             -> dst=0x{dst_slot:04x} dst_depth={dst_depth} OK",
            src_slot = src_slot,
            depth = depth_word,
            dst_slot = tcb_copy_slot,
            dst_depth = depth_word,
        )
    } else {
        write!(
            &mut copy_line,
            "[cnode:copy] src=seL4_CapInitThreadTCB slot=0x{src_slot:04x} src_depth={depth} \
             -> dst=0x{dst_slot:04x} dst_depth={dst_depth} ERR={err}",
            src_slot = src_slot,
            depth = depth_word,
            dst_slot = tcb_copy_slot,
            dst_depth = depth_word,
            err = copy_err as i32,
        )
    };
    if log_result.is_err() {
        // Partial diagnostics are acceptable.
    }
    force_uart_line(copy_line.as_str());
    if copy_err != sel4_sys::seL4_NoError {
        return Err(copy_err);
    }
    bump_slot(&mut dest);

    let captable_slot = match cs.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(
                &mut line,
                "[cspace:init] captable slot alloc err={}",
                err as i32,
            )
            .is_err()
            {}
            force_uart_line(line.as_str());
            return Err(err);
        }
    };
    dest.set_slot_offset(captable_slot);
    let captable_err = retype_captable(ut_cap as sel4_sys::seL4_Word, 4, &dest);
    if captable_err != sel4_sys::seL4_NoError {
        return Err(captable_err);
    }
    bump_slot(&mut dest);

    Ok(FirstRetypeResult {
        tcb_copy_slot,
        endpoint_slot,
        captable_slot,
    })
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
    /// Canonical root capability that can access kernel-seeded slots below `empty_start`.
    pub canonical_root_cap: sel4::seL4_CPtr,
    /// Slot index containing a copied init TCB capability once `smoke_copy_init_tcb` succeeds.
    pub tcb_copy_slot: sel4::seL4_CPtr,
    /// Slot index containing a copied root CNode capability when minted.
    pub root_cnode_copy_slot: sel4::seL4_CPtr,
    /// Destination CNode selected for subsequent retype operations.
    pub dest: DestCNode,
    /// Tracks whether the init CNode writable preflight succeeded.
    init_cnode_preflighted: bool,
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
        let init_cspace_root = cspace.root();
        assert_eq!(
            init_cspace_root, seL4_CapInitThreadCNode,
            "init CSpace root must match seL4_CapInitThreadCNode",
        );

        let init_cnode_bits = cspace.depth();
        let boot_bits = bi.init_cnode_bits();
        assert_eq!(
            init_cnode_bits, boot_bits,
            "CSpace depth and bootinfo init bits must align"
        );
        assert!(
            init_cnode_bits > 0,
            "bootinfo reported zero-width init CNode"
        );
        let word_bits = WORD_BITS as usize;
        assert!(
            init_cnode_bits as usize <= word_bits,
            "init CNode width {init} exceeds WordBits {word_bits}",
            init = init_cnode_bits,
        );
        // Init-root invocations must supply the bootinfo-advertised guard depth.
        let invocation_depth_bits = sel4::word_bits() as u8;
        let (first_free, last_free) = bi.init_cnode_empty_range();
        assert!(
            first_free < last_free,
            "bootinfo empty window must not be empty"
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
        let canonical_root_cap = bi.canonical_root_cap();
        let root_cnode_cap = init_cspace_root;
        let dest = make_root_dest(bi.header());
        let ctx = Self {
            bi,
            cspace,
            cnode_invocation_depth_bits: invocation_depth_bits,
            init_cnode_bits,
            first_free,
            last_free,
            root_cnode_cap,
            canonical_root_cap,
            tcb_copy_slot: sel4::seL4_CapNull,
            root_cnode_copy_slot: sel4::seL4_CapNull,
            dest,
            init_cnode_preflighted: false,
        };
        log_boot(first_free, last_free, init_cnode_bits);
        ctx
    }

    #[inline(always)]
    /// Returns the depth (in bits) of the init thread's root CNode.
    pub fn cnode_bits(&self) -> u8 {
        self.init_cnode_bits
    }

    /// Updates the destination CNode used for subsequent retype operations.
    pub fn set_dest(&mut self, dest: DestCNode) {
        dest.assert_sane();
        self.dest = dest;
    }

    #[inline(always)]
    fn slot_in_bounds(&self, slot: sel4::seL4_CPtr) -> bool {
        slot_in_empty_window(slot, self.first_free, self.last_free)
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
    pub fn alloc_slot_checked(&mut self) -> Result<sel4::seL4_CPtr, sel4::seL4_Error> {
        loop {
            let candidate = self.cspace.next_free_slot();
            cspace_sys::check_slot_in_range(self.init_cnode_bits, candidate);
            if !self.slot_in_bounds(candidate) {
                let failure = SlotAllocError::OutOfBootWindow {
                    candidate,
                    start: self.first_free,
                    end: self.last_free,
                };
                self.log_slot_failure(failure);
                return Err(sel4::seL4_RangeError);
            }

            let slot = match self.cspace.alloc_slot() {
                Ok(slot) => slot,
                Err(_) => {
                    let failure = SlotAllocError::OutOfBootWindow {
                        candidate,
                        start: self.first_free,
                        end: self.last_free,
                    };
                    self.log_slot_failure(failure);
                    return Err(sel4::seL4_RangeError);
                }
            };

            if Self::is_reserved_slot(slot) {
                self.log_slot_failure(SlotAllocError::ReservedSlot { slot });
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

    fn log_direct_init_path(&self, dst_slot: sel4::seL4_CPtr) {
        let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
        let guard_depth = self.dest.root_bits;
        if write!(
            &mut line,
            "[retype] path=direct:init-cnode dest=0x{dst_slot:04x} guard_depth={} root_bits={} window=[0x{start:04x}..0x{end:04x})",
            guard_depth,
            self.dest.root_bits,
            start = self.dest.empty_start,
            end = self.dest.empty_end,
        )
        .is_err()
        {
            // Truncated diagnostic; continue with partial line.
        }
        emit_console_line(line.as_str());
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
            if write!(
                &mut line,
                "[cnode] Copy err={err} root=0x{root:04x} dest(slot=0x{dest_index:04x},depth={depth}) src(slot=0x{src_index:04x},depth={depth})",
                root = self.root_cnode_cap,
                depth = self.init_cnode_bits,
                dest_index = dest_index,
                src_index = src_index,
            )
            .is_err()
            {
                // Partial diagnostics are acceptable.
            }
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
            if write!(
                &mut line,
                "[cnode] Mint err={err} root=0x{root:04x} dest(slot=0x{dest_index:04x},depth={depth}) src(slot=0x{src_index:04x},depth={depth}) badge={badge}",
                root = self.root_cnode_cap,
                depth = self.init_cnode_bits,
                dest_index = dest_index,
                src_index = src_index,
                badge = badge,
            )
            .is_err()
            {
                // Partial diagnostics are acceptable.
            }
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
        dest: &DestCNode,
    ) {
        if err != sel4::seL4_NoError {
            let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
            if write!(
                &mut line,
                "[retype] path={path} err={err} root=0x{root:04x} untyped_slot=0x{untyped:04x} node(idx=0,depth=0,off=0x{node_offset:04x}) dest_slot=0x{dest_index:04x} ty={obj_ty} sz={size_bits} window=[0x{start:04x}..0x{end:04x}) root_bits={bits}",
                path = dest.path_label(),
                root = dest.root,
                node_offset = dest.slot_offset,
                start = dest.empty_start,
                end = dest.empty_end,
                bits = dest.root_bits,
            )
            .is_err()
            {
                // Partial diagnostics are acceptable.
            }
            emit_console_line(line.as_str());
        }
    }

    /// Copies the init thread TCB capability into the free slot window to validate CSpace operations.
    pub fn smoke_copy_init_tcb(&mut self) -> Result<(), sel4::seL4_Error> {
        let dst_slot = self.alloc_slot_checked()?;
        let src_slot = seL4_CapInitThreadTCB;
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
            if write!(
                &mut line,
                "[cnode] Copy attempt dest=0x{dst_slot:04x} src=0x{src_slot:04x}",
            )
            .is_err()
            {
                // Partial diagnostics are acceptable.
            }
            emit_console_line(line.as_str());
        }
        let rights = cap_rights_read_write_grant();
        let err = self
            .cspace
            .copy_here(dst_slot, self.canonical_root_cap, src_slot, rights);
        self.log_cnode_copy(err, dst_slot, src_slot);
        err
    }

    /// Mints a duplicate of the root CNode capability for later management operations.
    pub fn mint_root_cnode_copy(&mut self) -> Result<(), sel4::seL4_Error> {
        let dst_slot = self.alloc_slot_checked()?;
        let src_slot = seL4_CapInitThreadCNode;
        let err = self.cspace.mint_here(
            dst_slot,
            self.canonical_root_cap,
            src_slot,
            sel4_sys::seL4_CapRights_All,
            0,
        );
        self.log_cnode_mint(err, dst_slot, src_slot, 0);
        if err == sel4::seL4_NoError {
            self.root_cnode_copy_slot = dst_slot;
            let init_ident = sel4::debug_cap_identify(seL4_CapInitThreadCNode);
            let copy_ident = sel4::debug_cap_identify(dst_slot);
            let init_rights = render_cap_rights(sel4_sys::seL4_CapRights_All);
            let copy_rights = render_cap_rights(sel4_sys::seL4_CapRights_All);
            ::log::info!(
                "[cnode] mint-success path=direct:init-cnode dest=0x{dst:04x} ident(init=0x{init_ident:08x},copy=0x{copy_ident:08x}) rights(init={init_rights},copy={copy_rights})",
                dst = dst_slot
            );
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
        let first_free = self.first_free;
        assert!(
            dst_slot >= first_free,
            "refusing to write below first_free (0x{first_free:04x})",
        );
        self.assert_slot_available(dst_slot);
        self.debug_identify_destinations();
        let mut dest = self.dest;
        dest.set_slot_offset(dst_slot);
        dest.assert_sane();
        self.log_direct_init_path(dst_slot);
        if !self.init_cnode_preflighted {
            if let Err(err) = cspace_sys::preflight_init_cnode_writable(dst_slot) {
                return err.into_sel4_error();
            }
            self.init_cnode_preflighted = true;
        }

        let err = super::retype::call_retype(untyped, obj_ty, size_bits, &dest, 1);
        self.log_retype(err, untyped, obj_ty, size_bits, dst_slot, &dest);
        if err == sel4::seL4_NoError {
            dest.bump_slot();
        }
        self.dest = dest;
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
            if write!(
                &mut line,
                "[cnode] op=SlotAlloc err=out_of_boot_window candidate=0x{candidate:04x} declared.empty=[0x{start:04x}..0x{end:04x}) runtime.empty=[0x{lo:04x}..0x{hi:04x})",
                candidate = candidate,
                start = start,
                end = end,
                lo = empty_start,
                hi = empty_end,
            )
            .is_err()
            {
                // Diagnostic truncation is acceptable.
            }
        }
        SlotAllocError::ReservedSlot { slot } => {
            if write!(
                &mut line,
                "[cnode] op=SlotAlloc err=reserved_slot slot=0x{slot:04x} runtime.empty=[0x{lo:04x}..0x{hi:04x})",
                slot = slot,
                lo = empty_start,
                hi = empty_end,
            )
            .is_err()
            {
                // Diagnostic truncation is acceptable.
            }
        }
    }

    emit_console_line(line.as_str());
}

fn emit_console_line(line: &str) {
    ::log::info!("{}", line);
    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}

fn render_cap_rights(rights: sel4_sys::seL4_CapRights) -> String<4> {
    let raw = rights.raw();
    let mut text = String::<4>::new();
    for (mask, glyph) in [(0x2, 'R'), (0x1, 'W'), (0x4, 'G'), (0x8, 'P')] {
        let ch = if (raw & mask) != 0 { glyph } else { '-' };
        let _ = text.push(ch);
    }
    text
}

#[inline(always)]
/// Returns `true` when the provided slot index lies within the bootinfo empty window.
pub fn slot_in_empty_window(
    idx: sel4::seL4_CPtr,
    start: sel4::seL4_CPtr,
    end: sel4::seL4_CPtr,
) -> bool {
    idx >= start && idx < end
}

#[inline(always)]
/// Returns the next slot index if advancing by one does not overflow.
pub fn slot_advance(idx: sel4::seL4_CPtr) -> Option<sel4::seL4_CPtr> {
    idx.checked_add(1)
}

#[cfg(feature = "sel4-debug")]
impl CSpaceCtx {
    fn debug_identify_destinations(&self) {
        let init_ident = sel4::debug_cap_identify(seL4_CapInitThreadCNode);
        let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
        let init_rights = render_cap_rights(sel4_sys::seL4_CapRights_All);
        if self.root_cnode_copy_slot != sel4::seL4_CapNull {
            let copy_ident = sel4::debug_cap_identify(self.root_cnode_copy_slot);
            let copy_rights = render_cap_rights(sel4_sys::seL4_CapRights_All);
            if write!(
                &mut line,
                "[retype] ident init=0x{init_ident:08x} rights(init={init_rights}) copy(slot=0x{slot:04x},tag=0x{copy_ident:08x},rights={copy_rights})",
                slot = self.root_cnode_copy_slot
            )
            .is_err()
            {
                // Diagnostic truncation is acceptable.
            }
        } else {
            if write!(
                &mut line,
                "[retype] ident init=0x{init_ident:08x} rights(init={init_rights}) copy=unavailable"
            )
            .is_err()
            {
                // Diagnostic truncation is acceptable.
            }
        }
        emit_console_line(line.as_str());
    }
}

#[cfg(not(feature = "sel4-debug"))]
impl CSpaceCtx {
    #[inline(always)]
    fn debug_identify_destinations(&self) {}
}
