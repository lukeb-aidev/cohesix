// Author: Lukas Bower
//! Thin wrappers around seL4 CSpace syscalls with argument validation helpers.
#![allow(unsafe_code)]

#[cfg(all(test, not(target_os = "none")))]
extern crate alloc;

use core::convert::TryFrom;
use core::fmt;

use crate::boot;
use crate::sel4;
use sel4_sys as sys;

#[cfg(target_os = "none")]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "none")]
static PREFLIGHT_COMPLETED: AtomicBool = AtomicBool::new(false);

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
    /// The node index must match the destination root CNode.
    NodeIndexMismatch {
        /// Node index supplied by the caller.
        provided: sys::seL4_CPtr,
        /// Expected node index (root CNode capability).
        expected: sys::seL4_CPtr,
    },
    /// Destination depth must match the init CNode depth.
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
                    "node_index 0x{provided:04x} must equal root 0x{expected:04x}",
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
    if args.node_index != expected_root {
        return Err(RetypeArgsError::NodeIndexMismatch {
            provided: args.node_index,
            expected: expected_root,
        });
    }
    let expected_depth =
        u8::try_from(bi_init_cnode_bits()).expect("initThreadCNodeSizeBits must fit within u8");
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

/// Depth (in bits) for a canonical single-level CNode.
pub const CANONICAL_CNODE_DEPTH_BITS: u8 = sys::seL4_WordBits as u8;

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
        let depth = u8::try_from(bits).expect("initThreadCNodeSizeBits must fit in u8");
        let err = unsafe {
            sys::seL4_CNode_Mint(
                root,
                probe_slot,
                depth,
                root,
                0,
                0,
                sel4::seL4_CapRights_All,
                0,
            )
        };
        if err != sys::seL4_NoError {
            ::log::error!(
                "preflight failed: Mint root=0x{root:04x} slot=0x{slot:04x} depth={} err={} ({})",
                depth,
                err,
                sel4::error_name(err),
                slot = probe_slot,
            );
            return Err(PreflightError::Probe(err));
        }

        let delete_err = unsafe { sys::seL4_CNode_Delete(root, probe_slot, depth) };
        if delete_err != sys::seL4_NoError {
            ::log::error!(
                "preflight cleanup failed: Delete root=0x{root:04x} slot=0x{slot:04x} depth={} err={} ({})",
                depth,
                delete_err,
                sel4::error_name(delete_err),
                slot = probe_slot,
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
    let init_bits = bi_init_cnode_bits();
    let capacity = if init_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << (init_bits as usize)
    };
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})",
    );
    let depth_bits = u8::try_from(init_bits).expect("initThreadCNodeSizeBits must fit within u8");
    let guard_depth = encode_cnode_depth(depth_bits);
    (bi_init_cnode_cptr(), slot as sys::seL4_Word, guard_depth, 0)
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
    let init_bits = bi_init_cnode_bits();
    let capacity = if init_bits as usize >= usize::BITS as usize {
        usize::MAX
    } else {
        1usize << (init_bits as usize)
    };
    debug_assert!(
        (slot as usize) < capacity,
        "slot 0x{slot:04x} exceeds init CNode capacity (limit=0x{capacity:04x})",
    );
    let root = bi_init_cnode_cptr();
    let depth_bits =
        u8::try_from(bi_init_cnode_bits()).expect("initThreadCNodeSizeBits must fit within u8");
    (
        root,
        root,
        encode_cnode_depth(depth_bits),
        slot as sys::seL4_Word,
    )
}

#[cfg(target_os = "none")]
#[inline(always)]
fn log_destination(op: &str, idx: sys::seL4_Word, depth: sys::seL4_Word, offset: sys::seL4_Word) {
    if boot::flags::trace_dest() {
        ::log::info!(
            "DEST → {op} root=0x{root:04x} idx=0x{idx:04x} depth={depth} off={offset} (ABI order: dest_root,dest_index,dest_depth,dest_offset)",
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

#[inline(always)]
/// Issues `seL4_CNode_Copy`, validating the target slot lies within the init CNode.
pub fn cnode_copy_direct_dest(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
    rights: sel4::SeL4CapRights,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = bi().initThreadCNodeSizeBits as u8;
        debug_assert_eq!(depth_bits, init_bits);
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        debug_assert_eq!(node_offset, 0);
        log_destination("CNode_Copy", node_index, node_depth, node_offset);
        let node_depth_u8 =
            u8::try_from(node_depth).expect("initThreadCNodeSizeBits must fit within u8");
        let err = unsafe {
            sys::seL4_CNode_Copy(
                root,
                node_index,
                node_depth_u8,
                src_root,
                src_index as sys::seL4_Word,
                src_depth_bits,
                rights,
            )
        };
        log_syscall_result("CNode_Copy", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: bi_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
            object_type: 0,
            size_bits: 0,
        });
        let _ = (src_root, src_index, src_depth_bits, rights);
        sys::seL4_NoError
    }
}

#[inline(always)]
/// Issues `seL4_CNode_Mint`, validating the target slot lies within the init CNode.
pub fn cnode_mint_direct_dest(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
    rights: sel4::SeL4CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = bi().initThreadCNodeSizeBits as u8;
        debug_assert_eq!(depth_bits, init_bits);
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        debug_assert_eq!(node_offset, 0);
        log_destination("CNode_Mint", node_index, node_depth, node_offset);
        let node_depth_u8 =
            u8::try_from(node_depth).expect("initThreadCNodeSizeBits must fit within u8");
        let err = unsafe {
            sys::seL4_CNode_Mint(
                root,
                node_index,
                node_depth_u8,
                src_root,
                src_index as sys::seL4_Word,
                src_depth_bits,
                rights,
                badge,
            )
        };
        log_syscall_result("CNode_Mint", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: bi_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
            object_type: 0,
            size_bits: 0,
        });
        let _ = (src_root, src_index, src_depth_bits, rights, badge);
        sys::seL4_NoError
    }
}

#[inline(always)]
/// Issues `seL4_CNode_Move`, validating the target slot lies within the init CNode.
pub fn cnode_move_direct_dest(
    depth_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let init_bits = bi().initThreadCNodeSizeBits as u8;
        debug_assert_eq!(depth_bits, init_bits);
        check_slot_in_range(depth_bits, dst_slot);
        let (root, node_index, node_depth, node_offset) = init_cnode_dest(dst_slot);
        debug_assert_eq!(node_offset, 0);
        log_destination("CNode_Move", node_index, node_depth, node_offset);
        let node_depth_u8 =
            u8::try_from(node_depth).expect("initThreadCNodeSizeBits must fit within u8");
        let err = unsafe {
            sys::seL4_CNode_Move(
                root,
                node_index,
                node_depth_u8,
                src_root,
                src_index as sys::seL4_Word,
                src_depth_bits,
            )
        };
        log_syscall_result("CNode_Move", err);
        err
    }

    #[cfg(not(target_os = "none"))]
    {
        let (node_index, node_depth, node_offset) =
            init_cnode_direct_destination_words(depth_bits, dst_slot);
        host_trace::record(host_trace::HostRetypeTrace {
            root: bi_init_cnode_cptr(),
            node_index,
            node_depth,
            node_offset,
            object_type: 0,
            size_bits: 0,
        });
        let _ = (src_root, src_index, src_depth_bits);
        sys::seL4_NoError
    }
}

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
    let depth_bits =
        u8::try_from(bi_init_cnode_bits()).expect("initThreadCNodeSizeBits must fit within u8");
    let expected_depth = encode_cnode_depth(depth_bits);
    debug_assert_eq!(root, sel4::seL4_CapInitThreadCNode);
    debug_assert_eq!(node_index, root);
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
        debug_assert_eq!(node_index, root);
        debug_assert_eq!(args.cnode_depth, depth_bits);
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
            node_depth: encode_cnode_depth(depth_bits),
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
    (
        dst_slot as sys::seL4_Word,
        encode_cnode_depth(init_cnode_bits),
        0,
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
        bi_init_cnode_cptr, init_cnode_dest, init_cnode_direct_destination_words_for_test,
        untyped_retype_into_init_root,
    };

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
        assert_eq!(idx, slot as _);
        let expected_depth = super::encode_cnode_depth(super::bi().initThreadCNodeSizeBits as u8);
        assert_eq!(depth, expected_depth);
        assert_eq!(off, 0);
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
                assert_eq!(trace.node_index, bi_init_cnode_cptr());
                let depth_bits = u8::try_from(super::bi_init_cnode_bits())
                    .expect("initThreadCNodeSizeBits must fit within u8");
                let expected_depth = super::encode_cnode_depth(depth_bits);
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
        assert_eq!(idx, bi_init_cnode_cptr());
        let depth_bits = u8::try_from(super::bi_init_cnode_bits())
            .expect("initThreadCNodeSizeBits must fit within u8");
        let expected_depth = super::encode_cnode_depth(depth_bits);
        assert_eq!(depth, expected_depth);
        assert_eq!(off, slot as _);
    }

    #[test]
    fn direct_destination_words_match_depth_bits() {
        let slot = 0x10u64;
        let bits = 13u8;
        let (idx, depth, off) = init_cnode_direct_destination_words_for_test(bits, slot as _);
        assert_eq!(idx, slot as _);
        assert_eq!(depth, super::encode_cnode_depth(bits));
        assert_eq!(off, 0);
    }

    #[test]
    fn validate_retype_args_accepts_canonical_call() {
        let empty_start = 0x100;
        let empty_end = 0x200;
        let depth_bits = u8::try_from(super::bi_init_cnode_bits())
            .expect("initThreadCNodeSizeBits must fit within u8");
        let args = super::RetypeArgs::new(
            0x80,
            0x20,
            12,
            bi_init_cnode_cptr(),
            bi_init_cnode_cptr(),
            super::encode_cnode_depth(depth_bits),
            0x180,
            1,
        );
        assert!(
            super::validate_retype_args(&args, empty_start, empty_end).is_ok(),
            "canonical args should validate"
        );
    }

    #[test]
    fn validate_retype_args_rejects_offset_before_window() {
        let empty_start = 0x120;
        let empty_end = 0x200;
        let depth_bits = u8::try_from(super::bi_init_cnode_bits())
            .expect("initThreadCNodeSizeBits must fit within u8");
        let args = super::RetypeArgs::new(
            0x90,
            0x30,
            10,
            bi_init_cnode_cptr(),
            bi_init_cnode_cptr(),
            super::encode_cnode_depth(depth_bits),
            empty_start - 1,
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
}
