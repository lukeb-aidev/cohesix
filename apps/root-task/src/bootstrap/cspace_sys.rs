// Author: Lukas Bower
//! Thin wrappers around seL4 CSpace syscalls with argument validation helpers.
#![allow(unsafe_code)]

#[cfg(all(test, not(target_os = "none")))]
extern crate alloc;

use core::convert::TryFrom;
use core::fmt;

use crate::boot;
use crate::bootstrap::cspace::{guard_root_path, CSpaceWindow};
#[cfg(target_os = "none")]
use crate::bootstrap::log::force_uart_line;
use crate::sel4;
use sel4_sys as sys;

#[cfg(target_os = "none")]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "none")]
static PREFLIGHT_COMPLETED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "none")]
const DEBUG_LOG_CAPACITY: usize = 224;

#[cfg(target_os = "none")]
fn debug_log(args: fmt::Arguments<'_>) {
    use core::fmt::Write;
    use heapless::String;

    let mut line = String::<DEBUG_LOG_CAPACITY>::new();
    let _ = line.write_fmt(args);
    force_uart_line(line.as_str());
}

#[cfg(not(target_os = "none"))]
fn debug_log(args: fmt::Arguments<'_>) {
    ::log::debug!("{}", args);
}

#[inline(always)]
fn log_window(tag: &str, window: &CSpaceWindow) {
    ::log::info!(
        "[cs] {tag}: root=0x{root:04x} bits={bits} first_free=0x{first:04x}",
        tag = tag,
        root = window.root,
        bits = window.bits,
        first = window.first_free,
    );
}

/// Retype a single endpoint object into the first free slot of the init CNode window.
pub fn retype_endpoint_once(
    untyped: sys::seL4_CPtr,
    window: &mut CSpaceWindow,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    log_window("win", window);
    let dest_root = window.root;
    let slot = window.first_free as sys::seL4_Word;
    let node_index: sys::seL4_Word = 0;
    let node_depth: sys::seL4_Word = 0;
    let node_offset: sys::seL4_Word = slot;
    let num_objects: sys::seL4_Word = 1;
    let size_bits: sys::seL4_Word = 0;

    #[cfg(target_os = "none")]
    let err = unsafe {
        sys::seL4_Untyped_Retype(
            untyped,
            sys::seL4_ObjectType::seL4_EndpointObject as sys::seL4_Word,
            size_bits,
            dest_root,
            node_index,
            node_depth,
            node_offset,
            num_objects,
        )
    };

    #[cfg(not(target_os = "none"))]
    let err = {
        host_trace::record(host_trace::HostRetypeTrace {
            root: dest_root,
            node_index,
            node_depth,
            node_offset,
            object_type: sys::seL4_ObjectType::seL4_EndpointObject as sys::seL4_Word,
            size_bits,
        });
        sys::seL4_NoError
    };

    if err == sys::seL4_NoError {
        window.bump();
        Ok(node_offset as sys::seL4_CPtr)
    } else {
        ::log::error!(
            "[boot:retype_ep] ut=0x{ut:04x} root=0x{root:04x} depth={depth} offset=0x{offset:04x} err={err:?}",
            ut = untyped,
            root = dest_root,
            depth = node_depth,
            offset = node_offset,
            err = err,
        );
        Err(err)
    }
}

/// Convert `initThreadCNodeSizeBits` into `u8` without panicking during
/// bring-up.
///
/// seL4 guarantees that this value is typically in the range 12–16. When an
/// unexpected value does slip through we log the anomaly and fall back to 13 so
/// that the system can continue booting deterministically.
#[inline(always)]
pub fn bits_as_u8(init_bits: usize) -> u8 {
    match u8::try_from(init_bits) {
        Ok(bits) => bits,
        Err(_) => {
            ::log::error!(
                "[cspace] initThreadCNodeSizeBits={} does not fit in u8; falling back to 13",
                init_bits
            );
            13
        }
    }
}

#[cfg(all(test, not(target_os = "none")))]
use alloc::boxed::Box;

/// Canonical ABI representation for `seL4_Untyped_Retype` arguments.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RetypeArgs {
    /// Capability pointer to the untyped object being retyped.
    pub ut: sys::seL4_CPtr,
    /// Numeric identifier describing the requested object type.
    pub objtype: u32,
    /// Size of the requested objects expressed as base-two logarithm.
    pub size_bits: u8,
    /// Padding reserved for ABI alignment.
    pub _pad0: [u8; 3],
    /// Capability pointer for the root CNode receiving the new objects.
    pub root: sys::seL4_CPtr,
    /// Index within the destination CNode.
    pub node_index: sys::seL4_CPtr,
    /// Depth (in bits) of the destination CNode pointer.
    pub cnode_depth: u8,
    /// Padding reserved for ABI alignment.
    pub _pad1: [u8; 7],
    /// Starting slot offset in the destination CNode.
    pub dest_offset: sys::seL4_CPtr,
    /// Number of objects to create.
    pub num_objects: u32,
    /// Padding reserved for ABI alignment.
    pub _pad2: u32,
}

impl RetypeArgs {
    /// Constructs a canonical argument block for `seL4_Untyped_Retype`.
    #[inline(always)]
    pub fn new(
        ut: sys::seL4_CPtr,
        objtype: sys::seL4_Word,
        size_bits: sys::seL4_Word,
        root: sys::seL4_CPtr,
        node_index: sys::seL4_CPtr,
        cnode_depth: sys::seL4_Word,
        dest_offset: sys::seL4_CPtr,
        num_objects: u32,
    ) -> Self {
        debug_assert!(size_bits <= u8::MAX as sys::seL4_Word);
        debug_assert!(cnode_depth <= u8::MAX as sys::seL4_Word);
        Self {
            ut,
            objtype: objtype as u32,
            size_bits: size_bits as u8,
            _pad0: [0; 3],
            root,
            node_index,
            cnode_depth: cnode_depth as u8,
            _pad1: [0; 7],
            dest_offset,
            num_objects,
            _pad2: 0,
        }
    }
}

/// Error raised when validating retype arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetypeArgsError {
    /// The supplied root CNode does not match `InitThreadCNode`.
    RootMismatch {
        /// Capability pointer provided by the caller.
        provided: sys::seL4_CPtr,
    },
    /// The node index must match the canonical guard index (zero).
    NodeIndexMismatch {
        /// Node index supplied by the caller.
        provided: sys::seL4_CPtr,
        /// Expected canonical guard index.
        expected: sys::seL4_CPtr,
    },
    /// Destination depth must match the canonical guard depth (zero).
    DepthMismatch {
        /// Depth supplied by the caller.
        provided: u8,
        /// Expected depth derived from bootinfo.
        expected: u8,
    },
    /// Destination offset fell outside the empty slot range.
    DestOffsetOutOfRange {
        /// Requested slot offset.
        offset: sys::seL4_CPtr,
        /// First slot in the empty range.
        empty_start: sys::seL4_CPtr,
        /// Exclusive end slot of the empty range.
        empty_end: sys::seL4_CPtr,
    },
    /// Caller attempted to retype zero objects.
    NumObjectsInvalid {
        /// Invalid object count.
        provided: u32,
    },
}

impl fmt::Display for RetypeArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMismatch { provided } => {
                write!(f, "root 0x{provided:04x} != InitThreadCNode")
            }
            Self::NodeIndexMismatch { provided, expected } => {
                write!(
                    f,
                    "node_index 0x{provided:04x} must equal canonical index 0x{expected:04x}",
                )
            }
            Self::DepthMismatch { provided, expected } => {
                write!(f, "cnode_depth {provided} must equal {expected} bits")
            }
            Self::DestOffsetOutOfRange {
                offset,
                empty_start,
                empty_end,
            } => write!(
                f,
                "dest_offset 0x{offset:04x} outside [{empty_start:04x}..{empty_end:04x})"
            ),
            Self::NumObjectsInvalid { provided } => {
                write!(f, "num_objects {provided} must be at least one")
            }
        }
    }
}

/// Errors produced while probing the init CNode for retype readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightError {
    /// Failure occurred while calling the probe path.
    Probe(sys::seL4_Error),
    /// Failure occurred while attempting to clean up probe state.
    Cleanup(sys::seL4_Error),
}

impl PreflightError {
    /// Converts the error into its underlying seL4 code.
    #[inline(always)]
    pub fn into_sel4_error(self) -> sys::seL4_Error {
        match self {
            Self::Probe(err) | Self::Cleanup(err) => err,
        }
    }
}

/// Errors surfaced by `untyped_retype_into_init_root`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetypeCallError {
    /// The provided arguments violated required invariants.
    Invariant(RetypeArgsError),
    /// The preflight probe detected an error.
    Preflight(PreflightError),
    /// The underlying kernel syscall failed.
    Kernel(sys::seL4_Error),
}

impl RetypeCallError {
    /// Converts the composite error into the seL4 status code exported to callers.
    #[inline(always)]
    pub fn into_sel4_error(self) -> sys::seL4_Error {
        match self {
            Self::Invariant(_) => sys::seL4_InvalidArgument,
            Self::Preflight(err) => err.into_sel4_error(),
            Self::Kernel(err) => err,
        }
    }
}

impl From<PreflightError> for RetypeCallError {
    fn from(err: PreflightError) -> Self {
        Self::Preflight(err)
    }
}

impl From<RetypeArgsError> for RetypeCallError {
    fn from(err: RetypeArgsError) -> Self {
        Self::Invariant(err)
    }
}

/// Emits a diagnostic trace of the supplied retype arguments.
pub fn log_retype_args(args: &RetypeArgs) {
    log::info!(
        "[retype] ut={:#x} type={:#x} size_bits={} root={:#x} idx={:#x} depth={} off={:#x} n={}",
        args.ut,
        args.objtype,
        args.size_bits,
        args.root,
        args.node_index,
        args.cnode_depth,
        args.dest_offset,
        args.num_objects
    );
}

/// Validates the canonical invariants for init CNode retypes.
///
/// The `empty_start` and `empty_end` parameters are raw slot indices within the init CNode.
pub fn validate_retype_args(
    args: &RetypeArgs,
    empty_start: sys::seL4_CPtr,
    empty_end: sys::seL4_CPtr,
) -> Result<(), RetypeArgsError> {
    let expected_root = sel4::seL4_CapInitThreadCNode;
    if args.root != expected_root {
        return Err(RetypeArgsError::RootMismatch {
            provided: args.root,
        });
    }
    if args.node_index != CANONICAL_CNODE_INDEX {
        return Err(RetypeArgsError::NodeIndexMismatch {
            provided: args.node_index,
            expected: CANONICAL_CNODE_INDEX,
        });
    }
    let init_bits_word = bi_init_cnode_bits();
    let expected_depth = bits_as_u8(init_bits_word as usize);
    if args.cnode_depth != expected_depth {
        return Err(RetypeArgsError::DepthMismatch {
            provided: args.cnode_depth,
            expected: expected_depth,
        });
    }
    if args.dest_offset < empty_start || args.dest_offset >= empty_end {
        return Err(RetypeArgsError::DestOffsetOutOfRange {
            offset: args.dest_offset,
            empty_start,
            empty_end,
        });
    }
    if args.num_objects == 0 {
        return Err(RetypeArgsError::NumObjectsInvalid {
            provided: args.num_objects,
        });
    }
    Ok(())
}

// Canonical source slot for the init thread's CSpace root.
// On real seL4 (target_os = "none"), this corresponds to seL4_CapInitThreadCNode.
#[cfg(target_os = "none")]
pub const CANONICAL_INIT_CNODE_SLOT: sys::seL4_CPtr =
    sys::seL4_CapInitThreadCNode as sys::seL4_CPtr;

// Host/mock builds rely on a fixed slot so unit tests remain stable.
#[cfg(not(target_os = "none"))]
pub const CANONICAL_INIT_CNODE_SLOT: sys::seL4_CPtr = 2;

// Backwards-compatible alias used by existing helpers.
pub const CANONICAL_CNODE_INDEX: sys::seL4_CPtr = CANONICAL_INIT_CNODE_SLOT;

#[cfg(target_os = "none")]
#[inline(always)]
fn bi() -> &'static sys::seL4_BootInfo {
    unsafe { &*sys::seL4_GetBootInfo() }
}

#[cfg(target_os = "none")]
#[inline(always)]
/// Returns the capability pointer for the init thread's root CNode.
pub fn bi_init_cnode_cptr() -> sys::seL4_CPtr {
    let root = sel4::seL4_CapInitThreadCNode;
    debug_assert_ne!(root, sys::seL4_CapNull, "init CNode root must be non-null");
    root
}

#[cfg(target_os = "none")]
#[inline(always)]
fn bi_init_cnode_bits() -> sys::seL4_Word {
    bi().initThreadCNodeSizeBits as sys::seL4_Word
}

#[cfg(target_os = "none")]
#[inline(always)]
fn boot_empty_window() -> (sys::seL4_CPtr, sys::seL4_CPtr) {
    (
        bi().empty.start as sys::seL4_CPtr,
        bi().empty.end as sys::seL4_CPtr,
    )
}

/// Quick probe to ensure the init CNode can accept write operations before issuing a Retype.
pub fn preflight_init_cnode_writable(probe_slot: sys::seL4_CPtr) -> Result<(), PreflightError> {
    let root = bi_init_cnode_cptr();
    let bits = bi_init_cnode_bits();
    debug_assert!(
        bits < sys::seL4_WordBits,
        "initThreadCNodeSizeBits must be less than seL4_WordBits",
    );
    let capacity = 1usize << (bits as usize);
    debug_assert!((probe_slot as usize) < capacity);

    #[cfg(all(debug_assertions, feature = "sel4-debug"))]
    unsafe {
        let ty = sys::seL4_DebugCapIdentify(root);
        debug_assert_eq!(
            ty,
            sys::seL4_ObjectType::seL4_CapTableObject as u32,
            "preflight: root 0x{root:x} is not a CNode (ty={ty})",
        );
    }

    #[cfg(target_os = "none")]
    {
        let depth = bits_as_u8(bits as usize);
        let probe_index = probe_slot as sys::seL4_CPtr;
        let src_index = CANONICAL_INIT_CNODE_SLOT as sys::seL4_CPtr;
        let err = unsafe {
            sys::seL4_CNode_Mint(
                root,
                probe_index,
                depth,
                root,
                src_index,
                depth,
                sel4::seL4_CapRights_All,
                0,
            )
        };
        if err != sys::seL4_NoError {
            ::log::error!(
                "preflight failed: Mint root=0x{root:04x} slot=0x{slot:04x} depth=initBits({depth}) err={} ({})",
                err,
                sel4::error_name(err),
                slot = probe_slot,
                depth = depth,
                root = root,
            );
            return Err(PreflightError::Probe(err));
        }

        let delete_err = unsafe { sys::seL4_CNode_Delete(root, probe_index, depth) };
        if delete_err != sys::seL4_NoError {
            ::log::error!(
                "preflight cleanup failed: Delete root=0x{root:04x} slot=0x{slot:04x} depth=initBits({depth}) err={} ({})",
                delete_err,
                sel4::error_name(delete_err),
                slot = probe_slot,
                depth = depth,
                root = root,
            );
            return Err(PreflightError::Cleanup(delete_err));
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = probe_slot;
    }

    Ok(())
}

#[cfg(all(test, not(target_os = "none")))]
static mut TEST_BOOTINFO_PTR: *const sys::seL4_BootInfo = core::ptr::null();

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn bi() -> &'static sys::seL4_BootInfo {
    unsafe {
        TEST_BOOTINFO_PTR
            .as_ref()
            .expect("test bootinfo not installed")
    }
}

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn bi_init_cnode_cptr() -> sys::seL4_CPtr {
    sel4::seL4_CapInitThreadCNode
}

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
fn bi_init_cnode_bits() -> sys::seL4_Word {
    bi().initThreadCNodeSizeBits as sys::seL4_Word
}

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
/// Installs a synthetic bootinfo pointer for host-based tests.
pub(crate) unsafe fn install_test_bootinfo_for_tests(
    bootinfo: sys::seL4_BootInfo,
) -> &'static sys::seL4_BootInfo {
    let leaked = Box::leak(Box::new(bootinfo));
    TEST_BOOTINFO_PTR = leaked as *const _;
    leaked
}

#[cfg(all(not(target_os = "none"), not(test)))]
#[inline(always)]
fn bi() -> &'static sys::seL4_BootInfo {
    panic!("bootinfo() unavailable on host targets");
}

#[cfg(all(not(target_os = "none"), not(test)))]
#[inline(always)]
fn bi_init_cnode_cptr() -> sys::seL4_CPtr {
    sys::seL4_CapInitThreadCNode
}

#[cfg(all(not(target_os = "none"), not(test)))]
#[inline(always)]
fn bi_init_cnode_bits() -> sys::seL4_Word {
    sys::seL4_WordBits as sys::seL4_Word
}

#[inline(always)]
/// Verifies that `slot` falls within the addressable range described by `init_cnode_bits`.
pub fn check_slot_in_range(init_cnode_bits: u8, slot: sys::seL4_CPtr) {
    let limit = if init_cnode_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_cnode_bits
    };
    assert!(
        (slot as usize) < limit,
        "slot {} out of range for init_cnode_bits={} (limit={})",
        slot,
        init_cnode_bits,
        limit
    );
}

#[inline(always)]
/// Encodes a radix depth in bits for syscall arguments expecting `seL4_Word`.
pub fn encode_cnode_depth(bits: u8) -> sys::seL4_Word {
    bits as sys::seL4_Word
}

/// Depth (in bits) used when traversing the init CNode for syscall arguments.
#[inline(always)]
fn init_cspace_depth_words(bi: &sys::seL4_BootInfo) -> sys::seL4_Word {
    sel4::init_cnode_bits(bi) as sys::seL4_Word
}

#[inline(always)]
fn slot_constant_label(slot: sys::seL4_Word) -> &'static str {
    match slot as sys::seL4_CPtr {
        x if x == sys::seL4_CapInitThreadCNode => "seL4_CapInitThreadCNode",
        x if x == sys::seL4_CapInitThreadTCB => "seL4_CapInitThreadTCB",
        x if x == sys::seL4_CapIRQControl => "seL4_CapIRQControl",
        x if x == sys::seL4_CapASIDControl => "seL4_CapASIDControl",
        _ => "-",
    }
}

fn root_constant_label(root: sys::seL4_CNode) -> &'static str {
    if root == sys::seL4_CapInitThreadCNode {
        "InitCNode"
    } else {
        "-"
    }
}

#[inline(always)]
pub fn cnode_copy_raw(
    bi: &sys::seL4_BootInfo,
    dst_root: sys::seL4_CNode,
    dst_slot_raw: sys::seL4_Word,
    src_root: sys::seL4_CNode,
    src_slot_raw: sys::seL4_Word,
    rights: sys::seL4_CapRights,
) -> sys::seL4_Error {
    let init_bits = sel4::init_cnode_bits(bi);
    let depth_bits = init_bits;
    let dst_label = slot_constant_label(dst_slot_raw);
    let src_label = slot_constant_label(src_slot_raw);
    let dst_root_label = root_constant_label(dst_root);
    let src_root_label = root_constant_label(src_root);
    let dst_index = dst_slot_raw;
    let src_index = src_slot_raw;

    ::log::info!(
        "[cnode-copy] dst_root={dst_root_label} dst_slot=0x{dst_slot_raw:04x} depth=initBits({depth}) ({dst_label})  \
         src_root={src_root_label} src_slot=0x{src_slot_raw:04x} depth=initBits({depth}) ({src_label})",
        depth = init_bits,
    );

    #[cfg(target_os = "none")]
    {
        unsafe {
            sys::seL4_CNode_Copy(
                dst_root,
                dst_index as sys::seL4_CPtr,
                depth_bits,
                src_root,
                src_index as sys::seL4_CPtr,
                depth_bits,
                rights,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (bi, dst_root, dst_slot_raw, src_root, src_slot_raw, rights);
        sys::seL4_NoError
    }
}

/// Probe 1: copy BootInfo cap into the first free slot.
#[cfg(feature = "cap-probes")]
#[allow(dead_code)]
pub fn probe_copy_bootinfo(bi: &sys::seL4_BootInfo) -> sys::seL4_Error {
    let dst = bi.empty.start as sys::seL4_CPtr;
    let src = sys::seL4_CapBootInfoFrame;
    ::log::info!("[probe] BootInfo -> 0x{dst:04x} (src=seL4_CapBootInfoFrame={src})");
    cnode_copy_raw(
        bi,
        sys::seL4_CapInitThreadCNode,
        dst as sys::seL4_Word,
        sys::seL4_CapInitThreadCNode,
        src as sys::seL4_Word,
        sys::seL4_CapRights_All,
    )
}

/// Probe 2: copy the init CNode cap into the next free slot.
#[cfg(feature = "cap-probes")]
#[allow(dead_code)]
pub fn probe_copy_cnode(bi: &sys::seL4_BootInfo) -> sys::seL4_Error {
    let dst = (bi.empty.start + 1) as sys::seL4_CPtr;
    let src = sys::seL4_CapInitThreadCNode;
    ::log::info!("[probe] CNode -> 0x{dst:04x} (src=seL4_CapInitThreadCNode={src})");
    cnode_copy_raw(
        bi,
        sys::seL4_CapInitThreadCNode,
        dst as sys::seL4_Word,
        sys::seL4_CapInitThreadCNode,
        src as sys::seL4_Word,
        sys::seL4_CapRights_All,
    )
}

/// Seed: copy the init thread TCB cap into the first free slot.
#[cfg(feature = "cap-probes")]
#[allow(dead_code)]
pub fn seed_copy_tcb_to_first_free(bi: &sys::seL4_BootInfo) -> sys::seL4_Error {
    let dst = bi.empty.start as sys::seL4_CPtr;
    let src = sys::seL4_CapInitThreadTCB;
    ::log::info!("[seed] TCB -> 0x{dst:04x} (src=seL4_CapInitThreadTCB={src})");
    cnode_copy_raw(
        bi,
        sys::seL4_CapInitThreadCNode,
        dst as sys::seL4_Word,
        sys::seL4_CapInitThreadCNode,
        src as sys::seL4_Word,
        sys::seL4_CapRights_All,
    )
}

#[inline(always)]
pub fn cnode_mint_raw(
    bi: &sys::seL4_BootInfo,
    dst_root: sys::seL4_CNode,
    dst_slot_raw: sys::seL4_Word,
    src_root: sys::seL4_CNode,
    src_slot_raw: sys::seL4_Word,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = sel4::init_cnode_bits(bi);
        let depth_bits = init_bits;
        let dst_index = dst_slot_raw;
        let src_index = src_slot_raw;
        unsafe {
            sys::seL4_CNode_Mint(
                dst_root,
                dst_index as sys::seL4_CPtr,
                depth_bits,
                src_root,
                src_index as sys::seL4_CPtr,
                depth_bits,
                rights,
                badge,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (
            bi,
            dst_root,
            dst_slot_raw,
            src_root,
            src_slot_raw,
            rights,
            badge,
        );
        sys::seL4_NoError
    }
}

#[inline(always)]
pub fn cnode_move_raw(
    bi: &sys::seL4_BootInfo,
    dst_root: sys::seL4_CNode,
    dst_slot_raw: sys::seL4_Word,
    src_root: sys::seL4_CNode,
    src_slot_raw: sys::seL4_Word,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = sel4::init_cnode_bits(bi);
        let depth_bits = init_bits;
        let dst_index = dst_slot_raw;
        let src_index = src_slot_raw;
        unsafe {
            sys::seL4_CNode_Move(
                dst_root,
                dst_index as sys::seL4_CPtr,
                depth_bits,
                src_root,
                src_index as sys::seL4_CPtr,
                depth_bits,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (bi, dst_root, dst_slot_raw, src_root, src_slot_raw);
        sys::seL4_NoError
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RootPath {
    root: sys::seL4_CNode,
    index: sys::seL4_Word,
    depth: sys::seL4_Word,
    offset: sys::seL4_Word,
    encoded_offset: sys::seL4_Word,
    init_bits: u8,
}

impl RootPath {
    #[inline(always)]
    pub(super) fn from_bootinfo(slot: u32, bi: &sys::seL4_BootInfo) -> Self {
        let init_bits = sel4::init_cnode_bits(bi);
        let root = sel4::init_cnode_cptr(bi);
        let index = root as sys::seL4_Word;
        let depth = init_bits as sys::seL4_Word;
        let offset = slot as sys::seL4_Word;
        guard_root_path(init_bits, index, depth, offset);
        let (empty_start, empty_end) = sel4::empty_window(bi);
        assert!(
            slot >= empty_start && slot < empty_end,
            "slot 0x{slot:04x} outside bootinfo empty window [0x{start:04x}..0x{end:04x})",
            start = empty_start,
            end = empty_end,
        );
        let encoded_offset = offset;
        Self {
            root,
            index,
            depth,
            offset,
            encoded_offset,
            init_bits,
        }
    }

    #[inline(always)]
    pub(super) fn encoded_offset(&self) -> sys::seL4_Word {
        self.encoded_offset
    }

    #[inline(always)]
    pub(super) fn offset(&self) -> sys::seL4_Word {
        self.offset
    }

    #[inline(always)]
    pub(super) fn init_bits(&self) -> u8 {
        self.init_bits
    }
}

#[inline(always)]
/// Constructs the `(root, index, depth, offset)` tuple targeting a slot in the init CNode.
pub fn init_cnode_dest(
    slot: sys::seL4_CPtr,
) -> (
    sys::seL4_CNode,
    sys::seL4_Word,
    sys::seL4_Word,
    sys::seL4_Word,
) {
    let init_bits_word = bi_init_cnode_bits();
    let init_bits_usize = init_bits_word as usize;
    let capacity = if init_bits_usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_bits_usize
    };
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})",
    );
    let root = bi_init_cnode_cptr();
    let offset = slot as sys::seL4_Word;
    let init_bits_u8 = bits_as_u8(init_bits_usize);
    debug_assert!(
        init_bits_u8 <= 31,
        "impossible cnode bits > 31 on AArch64: {init_bits_usize}",
    );
    ::log::info!(
        "[cspace] initBits(raw)={} -> (u8)={}",
        init_bits_usize,
        init_bits_u8
    );
    let guard_depth = init_bits_u8 as sys::seL4_Word;
    guard_root_path(init_bits_u8, root as sys::seL4_Word, guard_depth, offset);
    let _word_bits = u8::try_from(sel4::WORD_BITS).expect("WORD_BITS must fit within u8");
    let raw_index = offset;
    (root, raw_index, guard_depth, offset)
}

#[inline(always)]
/// Constructs the destination tuple for retype calls targeting the init CNode.
pub fn init_cnode_retype_dest(
    slot: sys::seL4_CPtr,
) -> (
    sys::seL4_CNode,
    sys::seL4_Word,
    sys::seL4_Word,
    sys::seL4_Word,
) {
    let init_bits_word = bi_init_cnode_bits();
    let init_bits_usize = init_bits_word as usize;
    let capacity = if init_bits_usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_bits_usize
    };
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})",
    );
    let root = bi_init_cnode_cptr();
    let node_index = 0;
    let node_depth = init_bits_word;
    let node_offset = slot as sys::seL4_Word;
    let depth_bits = bits_as_u8(init_bits_usize);
    debug_assert!(
        depth_bits <= 31,
        "impossible cnode bits > 31 on AArch64: {init_bits_usize}",
    );
    check_slot_in_range(depth_bits, slot);
    (root, node_index, node_depth, node_offset)
}

#[doc(hidden)]
pub use bits_as_u8 as super_bits_as_u8_for_test;

pub mod canonical {
    #[cfg(not(target_os = "none"))]
    use super::host_trace;
    use super::{bits_as_u8, debug_log, sel4, sys, RootPath};

    #[inline(always)]
    fn build_root_path(slot: u32, bi: &sys::seL4_BootInfo) -> RootPath {
        RootPath::from_bootinfo(slot, bi)
    }

    #[inline(always)]
    fn log_path(prefix: &str, slot: u32, path: &RootPath) {
        debug_log(format_args!(
            "{prefix} dst=0x{slot:04x} (root=0x{root:x} index=0x{index:x} depth={depth} offset=0x{raw:04x})",
            prefix = prefix,
            slot = slot,
            root = path.root,
            index = path.index,
            depth = path.depth,
            raw = path.offset(),
        ));
    }

    #[inline(always)]
    pub fn cnode_copy_into_root(
        dst_slot: u32,
        _src_root: sys::seL4_CNode,
        _src_index: sys::seL4_CPtr,
        _src_depth_bits: u8,
        _rights: sel4::SeL4CapRights,
        bi: &sys::seL4_BootInfo,
    ) -> Result<(), sys::seL4_Error> {
        let path = build_root_path(dst_slot, bi);
        log_path("[cnode:copy]", dst_slot, &path);

        let dst_root = sys::seL4_CapInitThreadCNode;
        let src_root = sys::seL4_CapInitThreadCNode;
        let raw_slot = path.offset();
        let src_slot = super::CANONICAL_INIT_CNODE_SLOT as sys::seL4_Word;
        let rights = sys::seL4_CapRights_All;
        let depth_word = super::init_cspace_depth_words(bi);
        let dst_label = super::slot_constant_label(raw_slot);
        let src_label = super::slot_constant_label(src_slot);

        #[cfg(target_os = "none")]
        let err = super::cnode_copy_raw(bi, dst_root, raw_slot, src_root, src_slot, rights);

        #[cfg(not(target_os = "none"))]
        let err = {
            host_trace::record(host_trace::HostRetypeTrace {
                root: dst_root,
                node_index: path.offset(),
                node_depth: depth_word,
                node_offset: 0,
                object_type: 0,
                size_bits: 0,
            });
            super::cnode_copy_raw(bi, dst_root, raw_slot, src_root, src_slot, rights)
        };

        ::log::info!(
            "[cnode.copy] dst_root=0x{dst_root:x} idx=0x{dst_index:x} ({dst_label}) depth={depth} \
             src_root=0x{src_root:x} idx=0x{src_index:x} ({src_label}) depth={depth} rights=0x{rights:x} -> err={err}",
            dst_root = dst_root,
            dst_index = raw_slot,
            dst_label = dst_label,
            src_root = src_root,
            src_index = src_slot,
            src_label = src_label,
            depth = depth_word,
            rights = rights.raw(),
            err = err,
        );

        if err == sys::seL4_NoError {
            Ok(())
        } else {
            Err(err)
        }
    }

    #[inline(always)]
    pub fn cnode_delete_from_root(
        dst_slot: u32,
        bi: &sys::seL4_BootInfo,
    ) -> Result<(), sys::seL4_Error> {
        let path = build_root_path(dst_slot, bi);
        log_path("[cnode:delete]", dst_slot, &path);

        let root = sys::seL4_CapInitThreadCNode;
        let raw_offset = path.offset();
        let idx = path.offset();
        let depth_word = super::init_cspace_depth_words(bi);
        let depth_bits = bits_as_u8(depth_word as usize);

        #[cfg(target_os = "none")]
        let err = unsafe { sys::seL4_CNode_Delete(root, idx as sys::seL4_CPtr, depth_bits) };

        #[cfg(not(target_os = "none"))]
        let err = sys::seL4_NoError;

        ::log::info!(
            "[cnode.delete] root=0x{root:x} slot=0x{slot:04x} idx=0x{idx:04x} depth=initBits({depth}) -> err={err}",
            root = root,
            slot = raw_offset,
            idx = idx,
            depth = depth_word,
            err = err,
        );

        if err == sys::seL4_NoError {
            Ok(())
        } else {
            Err(err)
        }
    }

    #[inline(always)]
    pub fn retype_into_root(
        ut: sys::seL4_CPtr,
        obj: u32,
        sz_bits: u32,
        dst_slot: u32,
        bi: &sys::seL4_BootInfo,
    ) -> Result<(), sys::seL4_Error> {
        let path = build_root_path(dst_slot, bi);
        debug_log(format_args!(
            "[retype:call] ut=0x{ut:x} obj={obj} sz={sz} -> (root,index,depth,raw)=(0x{root:x},{index},{depth},0x{raw:04x})",
            ut = ut,
            obj = obj,
            sz = sz_bits,
            root = path.root,
            index = path.index,
            depth = path.depth,
            raw = path.offset(),
        ));

        let root = sys::seL4_CapInitThreadCNode;
        let depth = 0;
        let offset = path.offset();
        let idx = 0;

        #[cfg(target_os = "none")]
        let err = unsafe {
            sys::seL4_Untyped_Retype(
                ut,
                obj as sys::seL4_Word,
                sz_bits as sys::seL4_Word,
                root,
                idx,
                depth,
                offset,
                1,
            )
        };

        #[cfg(not(target_os = "none"))]
        let err = {
            host_trace::record(host_trace::HostRetypeTrace {
                root,
                node_index: idx,
                node_depth: depth,
                node_offset: offset,
                object_type: obj as sys::seL4_Word,
                size_bits: sz_bits as sys::seL4_Word,
            });
            sys::seL4_NoError
        };

        ::log::info!(
            "[retype] ut=0x{ut:x} obj={obj} sz={sz_bits} root=0x{root:x} idx=0x{idx:04x} depth={depth} offset=0x{offset:04x} -> err={err}",
            ut = ut,
            obj = obj,
            sz_bits = sz_bits,
            root = root,
            idx = idx,
            offset = offset,
            depth = depth,
            err = err,
        );

        if err == sys::seL4_NoError {
            Ok(())
        } else {
            Err(err)
        }
    }
}

#[cfg(feature = "canonical_cspace")]
pub fn cnode_copy_into_root(dst_slot: u32, bi: &sys::seL4_BootInfo) -> Result<(), sys::seL4_Error> {
    let rights = crate::cspace::cap_rights_read_write_grant();
    let depth_bits = sel4::init_cnode_bits(bi);
    canonical::cnode_copy_into_root(
        dst_slot,
        sys::seL4_CapInitThreadCNode,
        sys::seL4_CapInitThreadTCB,
        depth_bits,
        rights,
        bi,
    )
}

/// Issues `seL4_Untyped_Retype` directly into the root CNode using canonical guard parameters.
pub fn retype_into_root(
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
    bi: &sys::seL4_BootInfo,
) -> Result<(), sys::seL4_Error> {
    let root = sys::seL4_CapInitThreadCNode;
    let depth = 0;
    let init_cnode_bits = sel4::init_cnode_bits(bi);
    let offset = dst_slot as sys::seL4_Word;
    let index = 0;

    let raw_index = sel4::init_cnode_cptr(bi) as sys::seL4_Word;
    guard_root_path(
        init_cnode_bits,
        raw_index,
        init_cnode_bits as sys::seL4_Word,
        offset,
    );

    #[cfg(all(target_os = "none", debug_assertions))]
    {
        let ident = unsafe { sys::seL4_DebugCapIdentify(root) };
        debug_log(format_args!(
            "[debug] Identify(root CNode) = {}",
            ident as u64
        ));
        let rc = unsafe {
            sys::seL4_CNode_Move(
                root,
                offset as sys::seL4_CPtr,
                init_cnode_bits,
                sys::seL4_CapNull,
                0,
                0,
            )
        };
        debug_log(format_args!("[probe:move-null] rc={}", rc as i32));
    }

    debug_log(format_args!(
        "[retype:call] ut={untyped:#06x} type={obj_type} size_bits={size_bits} root={root:#06x} index=0x{index:04x} depth={depth} offset=0x{offset:04x}",
        untyped = untyped,
        obj_type = obj_type,
        size_bits = size_bits,
        root = root,
        index = index,
        depth = depth,
        offset = offset,
    ));

    #[cfg(target_os = "none")]
    {
        log_destination("Untyped_Retype", index, depth, offset);
    }

    #[cfg(not(target_os = "none"))]
    {
        host_trace::record(host_trace::HostRetypeTrace {
            root,
            node_index: index,
            node_depth: depth,
            node_offset: offset,
            object_type: obj_type,
            size_bits,
        });
    }

    #[cfg(target_os = "none")]
    let err = unsafe {
        sys::seL4_Untyped_Retype(untyped, obj_type, size_bits, root, index, depth, offset, 1)
    };

    #[cfg(not(target_os = "none"))]
    let err = sys::seL4_NoError;

    if err == sys::seL4_NoError {
        debug_log(format_args!("[retype:ret] ok"));
        #[cfg(target_os = "none")]
        {
            log_syscall_result("Untyped_Retype", err);
        }
        return Ok(());
    }

    debug_log(format_args!("[retype:ret] err={}", err as i32));

    Err(err)
}

#[cfg(target_os = "none")]
#[inline(always)]
fn log_destination(op: &str, idx: sys::seL4_Word, depth: sys::seL4_Word, offset: sys::seL4_Word) {
    if boot::flags::trace_dest() {
        ::log::info!(
            "DEST → {op} root=0x{root:04x} idx=0x{idx:04x} depth={depth} off=0x{offset:04x} (ABI order: dest_root,dest_index,dest_depth,dest_offset)",
            op = op,
            root = bi_init_cnode_cptr(),
            idx = idx,
            depth = depth,
            offset = offset,
        );
    }
}

#[cfg(target_os = "none")]
#[inline(always)]
fn log_syscall_result(op: &str, err: sys::seL4_Error) {
    if boot::flags::trace_dest() {
        ::log::info!(
            "DEST ← {op} result={err} ({name})",
            op = op,
            err = err,
            name = sel4::error_name(err),
        );
    }
    if err != sys::seL4_NoError {
        ::log::error!(
            "{op} failed: err={err} ({name})",
            op = op,
            err = err,
            name = sel4::error_name(err),
        );
        panic!(
            "{op} failed with seL4 error {err} ({name})",
            op = op,
            err = err,
            name = sel4::error_name(err),
        );
    }
}

#[cfg(not(target_os = "none"))]
#[inline(always)]
fn log_destination(
    _op: &str,
    _idx: sys::seL4_Word,
    _depth: sys::seL4_Word,
    _offset: sys::seL4_Word,
) {
}

#[cfg(not(target_os = "none"))]
#[inline(always)]
fn log_syscall_result(_op: &str, _err: sys::seL4_Error) {}

#[cfg(not(target_os = "none"))]
mod host_trace {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::sys;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct HostRetypeTrace {
        pub root: sys::seL4_CNode,
        pub node_index: sys::seL4_Word,
        pub node_depth: sys::seL4_Word,
        pub node_offset: sys::seL4_Word,
        pub object_type: sys::seL4_Word,
        pub size_bits: sys::seL4_Word,
    }

    static LAST_ROOT: AtomicUsize = AtomicUsize::new(0);
    static LAST_INDEX: AtomicUsize = AtomicUsize::new(0);
    static LAST_DEPTH: AtomicUsize = AtomicUsize::new(0);
    static LAST_OFFSET: AtomicUsize = AtomicUsize::new(0);
    static LAST_OBJECT_TYPE: AtomicUsize = AtomicUsize::new(0);
    static LAST_SIZE_BITS: AtomicUsize = AtomicUsize::new(0);
    static HAS_TRACE: AtomicBool = AtomicBool::new(false);

    #[inline(always)]
    pub fn record(trace: HostRetypeTrace) {
        LAST_ROOT.store(trace.root as usize, Ordering::SeqCst);
        LAST_INDEX.store(trace.node_index as usize, Ordering::SeqCst);
        LAST_DEPTH.store(trace.node_depth as usize, Ordering::SeqCst);
        LAST_OFFSET.store(trace.node_offset as usize, Ordering::SeqCst);
        LAST_OBJECT_TYPE.store(trace.object_type as usize, Ordering::SeqCst);
        LAST_SIZE_BITS.store(trace.size_bits as usize, Ordering::SeqCst);
        HAS_TRACE.store(true, Ordering::SeqCst);
    }

    #[inline(always)]
    pub fn take_last() -> Option<HostRetypeTrace> {
        if !HAS_TRACE.swap(false, Ordering::SeqCst) {
            return None;
        }

        Some(HostRetypeTrace {
            root: LAST_ROOT.load(Ordering::SeqCst) as sys::seL4_CNode,
            node_index: LAST_INDEX.load(Ordering::SeqCst) as sys::seL4_Word,
            node_depth: LAST_DEPTH.load(Ordering::SeqCst) as sys::seL4_Word,
            node_offset: LAST_OFFSET.load(Ordering::SeqCst) as sys::seL4_Word,
            object_type: LAST_OBJECT_TYPE.load(Ordering::SeqCst) as sys::seL4_Word,
            size_bits: LAST_SIZE_BITS.load(Ordering::SeqCst) as sys::seL4_Word,
        })
    }
}

#[cfg(all(feature = "kernel", not(target_os = "none")))]
pub use host_trace::{take_last as take_last_host_retype_trace, HostRetypeTrace};

/// Retype explicitly into the init thread's CSpace root using guard-depth semantics.
pub fn untyped_retype_into_init_root(
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> Result<(), RetypeCallError> {
    let init_bits = bi_init_cnode_bits();
    let slot_capacity = if init_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << (init_bits as usize)
    };
    assert!(
        (dst_slot as usize) < slot_capacity,
        "destination slot 0x{dst:04x} exceeds init CNode capacity (limit=0x{limit:04x})",
        dst = dst_slot,
        limit = slot_capacity,
    );

    #[cfg(target_os = "none")]
    {
        let (empty_start, empty_end) = boot_empty_window();
        assert!(
            dst_slot >= empty_start,
            "refusing to write below first_free (0x{first_free:04x})",
            first_free = empty_start,
        );
        assert!(
            dst_slot < empty_end,
            "destination slot 0x{dst:04x} outside boot empty window [0x{lo:04x}..0x{hi:04x})",
            dst = dst_slot,
            lo = empty_start,
            hi = empty_end,
        );
    }

    let (root, node_index, node_depth, node_offset) = init_cnode_retype_dest(dst_slot);
    let expected_depth = bi_init_cnode_bits();
    debug_assert_eq!(root, sel4::seL4_CapInitThreadCNode);
    debug_assert_eq!(node_index, CANONICAL_CNODE_INDEX as sys::seL4_Word);
    debug_assert_eq!(node_depth, expected_depth);
    let args = RetypeArgs::new(
        untyped_slot,
        obj_type,
        size_bits,
        root,
        node_index,
        node_depth,
        node_offset,
        1,
    );

    log_retype_args(&args);

    #[cfg(target_os = "none")]
    {
        let (empty_start, empty_end) = boot_empty_window();
        debug_assert_eq!(node_index, CANONICAL_CNODE_INDEX as sys::seL4_Word);
        let expected_depth_bits = bits_as_u8(expected_depth as usize);
        debug_assert_eq!(args.cnode_depth, expected_depth_bits);
        debug_assert!(args.dest_offset >= empty_start && args.dest_offset < empty_end);

        validate_retype_args(&args, empty_start, empty_end)?;

        if !PREFLIGHT_COMPLETED.load(Ordering::Acquire) {
            preflight_init_cnode_writable(dst_slot)?;
            PREFLIGHT_COMPLETED.store(true, Ordering::Release);
        }

        let num_objects =
            usize::try_from(args.num_objects).expect("num_objects must fit within usize");
        let err = unsafe {
            sys::seL4_Untyped_Retype(
                args.ut,
                obj_type,
                size_bits,
                args.root,
                args.node_index,
                node_depth,
                args.dest_offset,
                num_objects,
            )
        };
        if err != sys::seL4_NoError {
            log::error!(
                "Untyped_Retype failed: err={err} ({name})",
                err = err,
                name = sel4::error_name(err),
            );
            return Err(RetypeCallError::Kernel(err));
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        host_trace::record(host_trace::HostRetypeTrace {
            root,
            node_index,
            node_depth,
            node_offset,
            object_type: obj_type,
            size_bits,
        });
        let _ = (untyped_slot, obj_type, size_bits);
    }

    Ok(())
}

#[inline(always)]
/// Retypes a RAM-backed untyped into the provided destination slot within the init CNode.
pub fn untyped_retype_into_cnode(
    dest_root: sys::seL4_CNode,
    depth_bits: u8,
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    let init_root = bi_init_cnode_cptr();
    if dest_root == init_root {
        return match untyped_retype_into_init_root(untyped_slot, obj_type, size_bits, dst_slot) {
            Ok(()) => sys::seL4_NoError,
            Err(err) => err.into_sel4_error(),
        };
    }

    #[cfg(target_os = "none")]
    {
        let node_depth = encode_cnode_depth(depth_bits);
        let err = unsafe {
            sys::seL4_Untyped_Retype(
                untyped_slot,
                obj_type,
                size_bits,
                dest_root,
                dst_slot as sys::seL4_Word,
                node_depth,
                0,
                1,
            )
        };
        log_syscall_result("Untyped_Retype", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        host_trace::record(host_trace::HostRetypeTrace {
            root: dest_root,
            node_index: dst_slot as sys::seL4_Word,
            node_depth: sys::seL4_WordBits as sys::seL4_Word,
            node_offset: 0,
            object_type: obj_type,
            size_bits,
        });
        let _ = (untyped_slot, obj_type, size_bits);
        sys::seL4_NoError
    }
}

#[cfg(any(test, not(target_os = "none")))]
#[inline(always)]
pub(crate) fn init_cnode_direct_destination_words(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
) -> (sys::seL4_Word, sys::seL4_Word, sys::seL4_Word) {
    let limit = if init_cnode_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << init_cnode_bits
    };
    debug_assert!(
        (dst_slot as usize) < limit,
        "destination slot {dst_slot:#x} exceeds init CNode capacity (bits={init_cnode_bits})"
    );
    let raw_index = dst_slot as sys::seL4_Word;
    (
        raw_index,
        init_cnode_bits as sys::seL4_Word,
        dst_slot as sys::seL4_Word,
    )
}

#[cfg(test)]
#[inline(always)]
pub fn init_cnode_direct_destination_words_for_test(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
) -> (sys::seL4_Word, sys::seL4_Word, sys::seL4_Word) {
    init_cnode_direct_destination_words(depth_bits, dst_slot)
}

#[cfg(test)]
mod tests {
    use core::convert::TryFrom;

    use super::{
        bi_init_cnode_bits, bi_init_cnode_cptr, canonical, init_cnode_dest,
        init_cnode_direct_destination_words_for_test, sel4, sys, untyped_retype_into_init_root,
    };

    #[cfg(not(target_os = "none"))]
    fn mock_bootinfo(empty_start: u32, empty_end: u32, bits: u8) -> sys::seL4_BootInfo {
        let mut bootinfo: sys::seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.initThreadCNodeSizeBits = bits as _;
        bootinfo.empty.start = empty_start as _;
        bootinfo.empty.end = empty_end as _;
        bootinfo
    }

    #[test]
    fn init_cnode_dest_radix_depth_is_valid() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            super::install_test_bootinfo_for_tests(bootinfo);
        }
        let slot = 0x00a5u64;
        let (root, idx, depth, off) = init_cnode_dest(slot as _);
        assert_eq!(root, bi_init_cnode_cptr());
        assert_eq!(idx, slot as sys::seL4_Word);
        let expected_depth = 13u64;
        assert_eq!(depth, expected_depth);
        assert_eq!(off, slot as sys::seL4_Word);
    }

    #[test]
    fn retype_into_init_root_uses_canonical_tuple() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            bootinfo.empty.start = 0x00a6;
            bootinfo.empty.end = 0x0800;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let slot = 0x00a6u64;
        #[cfg(not(target_os = "none"))]
        while super::host_trace::take_last().is_some() {}

        let err = untyped_retype_into_init_root(0, 0, 0, slot as _);
        assert!(err.is_ok());

        #[cfg(not(target_os = "none"))]
        {
            if let Some(trace) = super::host_trace::take_last() {
                assert_eq!(trace.root, bi_init_cnode_cptr());
                assert_eq!(trace.node_index, super::CANONICAL_CNODE_INDEX as _);
                let expected_depth = bi_init_cnode_bits();
                assert_eq!(trace.node_depth, expected_depth);
                assert_eq!(trace.node_offset, slot as _);
                assert_eq!(trace.object_type, 0);
                assert_eq!(trace.size_bits, 0);
            } else {
                panic!("expected host trace for init-root retype");
            }
        }
    }

    #[test]
    fn init_cnode_retype_dest_matches_canonical_tuple() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let slot = 0x10u64;
        let (root, idx, depth, off) = super::init_cnode_retype_dest(slot as _);
        assert_eq!(root, bi_init_cnode_cptr());
        assert_eq!(idx, super::CANONICAL_CNODE_INDEX as _);
        let expected_depth = bi_init_cnode_bits();
        assert_eq!(depth, expected_depth);
        assert_eq!(off, slot as _);
    }

    #[test]
    fn direct_destination_words_match_depth_bits() {
        let slot = 0x10u64;
        let bits = 13u8;
        let (idx, depth, off) = init_cnode_direct_destination_words_for_test(bits, slot as _);
        let expected_index = encode_slot(slot, bits);
        assert_eq!(idx, expected_index);
        assert_eq!(depth, bits as sys::seL4_Word);
        assert_eq!(off, slot as _);
    }

    #[test]
    fn validate_retype_args_accepts_canonical_call() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let empty_start = 0x100;
        let empty_end = 0x200;
        let dest_offset = 0x180;
        let args = super::RetypeArgs::new(
            0x80,
            0x20,
            12,
            bi_init_cnode_cptr(),
            super::CANONICAL_CNODE_INDEX,
            bi_init_cnode_bits(),
            dest_offset,
            1,
        );
        assert!(
            super::validate_retype_args(&args, empty_start, empty_end).is_ok(),
            "canonical args should validate"
        );
    }

    #[test]
    fn validate_retype_args_rejects_offset_before_window() {
        #[cfg(not(target_os = "none"))]
        unsafe {
            let mut bootinfo: sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 13;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let empty_start_raw = 0x120u64;
        let empty_end = 0x200u64;
        let empty_start = empty_start_raw;
        let encoded_before_window = empty_start_raw - 1;
        let args = super::RetypeArgs::new(
            0x90,
            0x30,
            10,
            bi_init_cnode_cptr(),
            super::CANONICAL_CNODE_INDEX,
            bi_init_cnode_bits(),
            encoded_before_window,
            1,
        );
        let err = super::validate_retype_args(&args, empty_start, empty_end)
            .expect_err("offset before window should fail");
        match err {
            super::RetypeArgsError::DestOffsetOutOfRange { .. } => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "debug assertions disabled")]
    fn init_cnode_dest_rejects_out_of_range_slot() {
        use std::panic;

        unsafe {
            let mut bootinfo: sys::seL4_BootInfo = core::mem::zeroed();
            bootinfo.initThreadCNodeSizeBits = 5;
            super::install_test_bootinfo_for_tests(bootinfo);
        }

        let limit_slot = 1usize << 5;
        let result = panic::catch_unwind(|| {
            let slot = limit_slot as sys::seL4_CPtr;
            let _ = init_cnode_dest(slot);
        });

        assert!(
            result.is_err(),
            "init_cnode_dest should panic when slot is out of range"
        );
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn canonical_root_path_accepts_empty_window_slot() {
        let bootinfo = mock_bootinfo(0x20, 0x40, 6);
        let path = super::RootPath::from_bootinfo(0x22, &bootinfo);
        assert_eq!(path.root, sys::seL4_CapInitThreadCNode);
        assert_eq!(path.index, sys::seL4_CapInitThreadCNode as _);
        assert_eq!(path.depth, 6);
        assert_eq!(path.offset(), 0x22);
        assert_eq!(path.encoded_offset(), 0x22);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn canonical_root_path_rejects_before_empty_window() {
        use std::panic;

        let bootinfo = mock_bootinfo(0x30, 0x50, 6);
        let result = panic::catch_unwind(|| {
            let _ = super::RootPath::from_bootinfo(0x2F, &bootinfo);
        });
        assert!(result.is_err(), "slot before empty window should panic");
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn canonical_copy_guard_blocks_out_of_window() {
        use std::panic;

        let bootinfo = mock_bootinfo(0x40, 0x60, 7);
        let rights = sel4_sys::seL4_CapRights::new(0, 1, 1, 1);
        let result = panic::catch_unwind(|| {
            let _ = canonical::cnode_copy_into_root(
                0x3F,
                sys::seL4_CapInitThreadCNode,
                0,
                0,
                rights,
                &bootinfo,
            );
        });
        assert!(
            result.is_err(),
            "copy should guard against slots before window"
        );
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn canonical_copy_allows_valid_slot() {
        let bootinfo = mock_bootinfo(0x80, 0xA0, 8);
        let rights = sel4_sys::seL4_CapRights::new(0, 1, 1, 1);
        let result = canonical::cnode_copy_into_root(
            0x85,
            sys::seL4_CapInitThreadCNode,
            0,
            0,
            rights,
            &bootinfo,
        );
        assert!(result.is_ok());
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn canonical_retype_records_host_trace() {
        while super::host_trace::take_last().is_some() {}
        let bootinfo = mock_bootinfo(0x100, 0x180, 8);
        let result = canonical::retype_into_root(0xAA, 4, 12, 0x120, &bootinfo);
        assert!(result.is_ok());
        let trace = super::host_trace::take_last().expect("expected host trace entry");
        assert_eq!(trace.root, sys::seL4_CapInitThreadCNode);
        assert_eq!(trace.node_index, sys::seL4_CapInitThreadCNode as _);
        assert_eq!(
            trace.node_depth,
            sel4::init_cnode_bits(&bootinfo) as sys::seL4_Word
        );
        assert_eq!(trace.node_offset, 0x120);
        assert_eq!(trace.object_type, 4);
        assert_eq!(trace.size_bits, 12);
    }
}
