// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: seL4 resource management helpers for the root task.
// Author: Lukas Bower
//! seL4 resource management helpers for the root task.
#![cfg(any(test, feature = "kernel"))]
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]
#![allow(unsafe_code)]

#[cfg(all(test, not(target_os = "none")))]
use crate::rust_alloc::boxed::Box;
use core::{
    convert::TryInto,
    fmt,
    fmt::Write,
    mem,
    ops::Range,
    panic::Location,
    ptr::{self, NonNull},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use spin::Mutex as SpinMutex;
#[cfg(all(feature = "kernel", target_os = "none"))]
use spin::Once as SpinOnce;

use crate::bootstrap::bootinfo_snapshot::{BootInfoState, BootinfoWindow};
use crate::bootstrap::cspace_sys;
use crate::bootstrap::ipcbuf_view::IpcBufView;
use crate::bootstrap::log as boot_log;
use crate::bootstrap::sel4_guard;
use crate::bootstrap::DevicePtPoolConfig;
use crate::debug::{watch_hint_for, watch_range};
use crate::debug_uart::debug_uart_str;
use crate::sel4_view;
use crate::serial;
#[cfg(all(test, not(feature = "kernel")))]
use heapless::Vec as HeaplessVec;
use heapless::{String as HeaplessString, Vec};
pub use sel4_sys::{
    seL4_AllRights, seL4_CNode, seL4_CNode_Copy, seL4_CNode_Delete, seL4_CNode_Mint,
    seL4_CNode_Move, seL4_CNode_Revoke, seL4_CPtr, seL4_CapASIDControl, seL4_CapBootInfoFrame,
    seL4_CapDomain, seL4_CapIOPort, seL4_CapIOSpace, seL4_CapIRQControl,
    seL4_CapInitThreadASIDPool, seL4_CapInitThreadCNode, seL4_CapInitThreadIPCBuffer,
    seL4_CapInitThreadSC, seL4_CapInitThreadTCB, seL4_CapInitThreadVSpace, seL4_CapNull,
    seL4_CapRights, seL4_CapRights_All, seL4_CapRights_ReadWrite, seL4_CapSMC,
    seL4_CapSMMUCBControl, seL4_CapSMMUSIDControl, seL4_DeleteFirst, seL4_Error, seL4_FailedLookup,
    seL4_GetBootInfo, seL4_MessageInfo, seL4_NoError, seL4_NotEnoughMemory, seL4_ObjectType,
    seL4_RangeError, seL4_Untyped, seL4_Untyped_Retype, seL4_Word,
};
use static_assertions::const_assert;

#[cfg(feature = "kernel")]
mod syscall;

/// Canonical capability rights representation exposed by seL4.
pub type SeL4CapRights = sel4_sys::seL4_CapRights;

/// Architectural word width (in bits) exposed by seL4.
pub const WORD_BITS: seL4_Word = sel4_sys::seL4_WordBits as seL4_Word;

/// Maximum number of message words carried by an seL4 IPC frame.
///
/// The value mirrors `seL4_MsgMaxLength` for the target kernel build. The
/// kernel artefacts bundled under `seL4/build/` advertise a 120-word bound for
/// `aarch64/virt`, matching the upstream default of 960 bytes per message.
pub const MSG_MAX_WORDS: usize = 120;

/// seL4 page bits for the configured kernel (4 KiB pages).
pub const IPC_PAGE_BITS: usize = 12;

/// Size in bytes of a single seL4 IPC buffer page.
pub const IPC_PAGE_BYTES: usize = 1 << IPC_PAGE_BITS;

/// Size in bytes of the fixed seL4 bootinfo frame.
///
/// The kernel guarantees `seL4_BootInfoFrameBits == seL4_PageBits` for this
/// configuration, so the bootinfo frame occupies exactly one page even though
/// the concrete `seL4_BootInfo` struct is smaller.
pub const BOOTINFO_FRAME_BYTES: usize = IPC_PAGE_BYTES;

const_assert!(sel4_sys::seL4_PageBits == 12);
const_assert!(BOOTINFO_FRAME_BYTES == IPC_PAGE_BYTES);
const CANONICAL_ROOT_SENTINEL: usize = usize::MAX;
static CANONICAL_ROOT_CAP: AtomicUsize =
    AtomicUsize::new(sel4_sys::seL4_CapInitThreadCNode as usize);
static CANONICAL_ROOT_SLOT: AtomicUsize = AtomicUsize::new(CANONICAL_ROOT_SENTINEL);
static EP_VALIDATED: AtomicBool = AtomicBool::new(false);
static IPC_SEND_UNLOCKED: AtomicBool = AtomicBool::new(false);
static BOOTINFO_WINDOW_DUMPED: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, not(target_os = "none")))]
static mut TEST_BOOTINFO_PTR: *const seL4_BootInfo = ptr::null();
const DMA_MAP_DIAG: bool = cfg!(feature = "dev-virt") || cfg!(feature = "cache-trace");
const DMA_MAP_LOG_CAPACITY: usize = 128;
const MAX_DEVICE_SKIP_OBJECTS: usize = 512;
const MAX_DEVICE_FRAME_CACHE: usize = 256;
const SEL4_UNTYPED_OBJECT_WORD: seL4_Word = sel4_sys::seL4_UntypedObject as seL4_Word;
const SEL4_TCB_OBJECT_WORD: seL4_Word = sel4_sys::seL4_TCBObject as seL4_Word;
const SEL4_ENDPOINT_OBJECT_WORD: seL4_Word = sel4_sys::seL4_EndpointObject as seL4_Word;
const SEL4_NOTIFICATION_OBJECT_WORD: seL4_Word = sel4_sys::seL4_NotificationObject as seL4_Word;
const SEL4_CAP_TABLE_OBJECT_WORD: seL4_Word = sel4_sys::seL4_CapTableObject as seL4_Word;
const SEL4_ARM_PAGE_OBJECT_WORD: seL4_Word = sel4_sys::seL4_ARM_SmallPageObject as seL4_Word;
const SEL4_ARM_LARGE_PAGE_OBJECT_WORD: seL4_Word =
    sel4_sys::seL4_ARM_LargePageObjectType as seL4_Word;
const SEL4_ARM_PAGE_TABLE_OBJECT_WORD: seL4_Word = sel4_sys::seL4_ARM_PageTableObject as seL4_Word;
const SEL4_ARM_VSPACE_OBJECT_WORD: seL4_Word = sel4_sys::seL4_ARM_VSpaceObject as seL4_Word;
// Local-seat DMA frame trace can generate thousands of UART lines during USB
// enumeration; keep it opt-in for targeted diagnostics.
const LOCAL_SEAT_DMA_FRAME_VERBOSE_LOGS: bool = false;

#[derive(Clone, Copy)]
struct DmaMapRecord {
    paddr: usize,
    vaddr: usize,
    len: usize,
    attr: usize,
}

static DMA_MAP_LOG: SpinMutex<Vec<DmaMapRecord, DMA_MAP_LOG_CAPACITY>> = SpinMutex::new(Vec::new());
static DMA_MAP_DROPS_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "none")]
#[inline(always)]
pub(crate) fn runtime_bootinfo() -> &'static seL4_BootInfo {
    unsafe { &*sel4_sys::seL4_GetBootInfo() }
}

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
pub(crate) fn runtime_bootinfo() -> &'static seL4_BootInfo {
    unsafe {
        TEST_BOOTINFO_PTR
            .as_ref()
            .expect("test bootinfo not installed")
    }
}

#[cfg(all(not(target_os = "none"), not(test)))]
#[inline(always)]
pub(crate) fn runtime_bootinfo() -> &'static seL4_BootInfo {
    panic!("runtime bootinfo unavailable on host targets");
}

#[cfg(all(test, not(target_os = "none")))]
#[inline(always)]
pub(crate) fn install_test_bootinfo_for_tests(
    bootinfo: seL4_BootInfo,
) -> &'static mut seL4_BootInfo {
    let leaked = Box::leak(Box::new(bootinfo));
    unsafe {
        TEST_BOOTINFO_PTR = leaked as *const _;
    }
    leaked
}

#[cfg(test)]
#[inline(always)]
pub(crate) fn blank_bootinfo_for_tests() -> seL4_BootInfo {
    seL4_BootInfo {
        extraLen: 0,
        nodeId: 0,
        numNodes: 0,
        numIOPTLevels: 0,
        ipcBuffer: ptr::null_mut(),
        empty: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        sharedFrames: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        userImageFrames: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        userImagePaging: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        ioSpaceCaps: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        extraBIPages: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        initThreadCNodeSizeBits: 0,
        _padding_init_cnode_bits: [0; core::mem::size_of::<seL4_Word>() - 1],
        initThreadDomain: 0,
        untyped: sel4_sys::seL4_SlotRegion { start: 0, end: 0 },
        untypedList: [sel4_sys::seL4_UntypedDesc {
            paddr: 0,
            sizeBits: 0,
            isDevice: 0,
            padding: [0; core::mem::size_of::<seL4_Word>() - 2],
        }; sel4_sys::MAX_BOOTINFO_UNTYPEDS],
    }
}

pub fn record_dma_mapping(paddr: usize, vaddr: usize, len: usize, attr: usize) {
    if !DMA_MAP_DIAG || len == 0 {
        return;
    }
    let mut log = DMA_MAP_LOG.lock();
    if log.len() == log.capacity() {
        if !DMA_MAP_DROPS_LOGGED.swap(true, Ordering::AcqRel) {
            log::warn!(
                target: "hal",
                "[hal][dma-map] mapping log full; dropping additional entries",
            );
        }
        return;
    }
    let _ = log.push(DmaMapRecord {
        paddr,
        vaddr,
        len,
        attr,
    });
}

pub fn dump_dma_mappings_for_range(paddr: usize, len: usize, label: &str) {
    if !DMA_MAP_DIAG || len == 0 {
        return;
    }
    let end = paddr.saturating_add(len);
    let log = DMA_MAP_LOG.lock();
    let mut hits = 0usize;
    for (idx, record) in log.iter().enumerate() {
        let rec_end = record.paddr.saturating_add(record.len);
        if paddr < rec_end && record.paddr < end {
            hits = hits.saturating_add(1);
            log::info!(
                target: "hal",
                "[hal][dma-map] {label} hit={hits} idx={idx} paddr=0x{paddr:016x}..0x{pend:016x} vaddr=0x{vaddr:016x}..0x{vend:016x} len=0x{len:x} attr=0x{attr:08x}",
                paddr = record.paddr,
                pend = rec_end,
                vaddr = record.vaddr,
                vend = record.vaddr.saturating_add(record.len),
                len = record.len,
                attr = record.attr,
            );
        }
    }
    if hits == 0 {
        log::warn!(
            target: "hal",
            "[hal][dma-map] {label} no mapping record for paddr=0x{paddr:016x} len=0x{len:x}",
            paddr = paddr,
            len = len,
        );
    } else if hits > 1 {
        log::warn!(
            target: "hal",
            "[hal][dma-map] {label} multiple mappings detected (hits={hits})",
        );
    }
}

/// Logs ABI sanity for key seL4 types to validate the Rust FFI surface.
pub fn log_sel4_type_sanity() {
    use core::mem::{align_of, size_of};

    log::info!(
        "[sel4-type-sanity] seL4_Word size={} align={} seL4_CNode size={} align={} seL4_Error size={} align={}",
        size_of::<sel4_sys::seL4_Word>(),
        align_of::<sel4_sys::seL4_Word>(),
        size_of::<sel4_sys::seL4_CNode>(),
        align_of::<sel4_sys::seL4_CNode>(),
        size_of::<sel4_sys::seL4_Error>(),
        align_of::<sel4_sys::seL4_Error>(),
    );

    log::info!(
        "[sel4-type-sanity] seL4_CapRights size={} align={} seL4_CPtr size={} align={}",
        size_of::<sel4_sys::seL4_CapRights_t>(),
        align_of::<sel4_sys::seL4_CapRights_t>(),
        size_of::<sel4_sys::seL4_CPtr>(),
        align_of::<sel4_sys::seL4_CPtr>(),
    );

    debug_assert_eq!(size_of::<sel4_sys::seL4_Word>(), 8);
    debug_assert_eq!(align_of::<sel4_sys::seL4_Word>(), 8);
}

#[inline(always)]
pub fn canonical_root_cap_ptr() -> seL4_CPtr {
    CANONICAL_ROOT_CAP.load(Ordering::Acquire) as seL4_CPtr
}

#[inline(always)]
pub fn publish_canonical_root_alias(alias_slot: seL4_CPtr) {
    debug_assert_ne!(alias_slot, seL4_CapNull, "canonical alias must not be null");
    CANONICAL_ROOT_CAP.store(alias_slot as usize, Ordering::Release);
    CANONICAL_ROOT_SLOT.store(alias_slot as usize, Ordering::Release);
}

#[inline(always)]
pub fn canonical_root_alias_slot() -> Option<seL4_CPtr> {
    let slot = CANONICAL_ROOT_SLOT.load(Ordering::Acquire);
    if slot == CANONICAL_ROOT_SENTINEL {
        None
    } else {
        Some(slot as seL4_CPtr)
    }
}

#[inline(always)]
pub fn reset_canonical_root_alias() {
    CANONICAL_ROOT_CAP.store(
        sel4_sys::seL4_CapInitThreadCNode as usize,
        Ordering::Release,
    );
    CANONICAL_ROOT_SLOT.store(CANONICAL_ROOT_SENTINEL, Ordering::Release);
}

/// Computes the canonical traversal depth (in bits) for addressing the init thread's CNode.
#[inline(always)]
pub const fn canonical_cnode_depth(init_bits: u8, word_bits: u8) -> u8 {
    assert!(
        init_bits as usize <= word_bits as usize,
        "initThreadCNodeSizeBits must not exceed word width",
    );
    word_bits
}

#[inline(always)]
#[track_caller]
pub fn canonical_cnode_bits(bi: &sel4_sys::seL4_BootInfo) -> u8 {
    let caller = Location::caller();
    let init_bits_raw = bi.initThreadCNodeSizeBits as usize;
    let word_bits = sel4_sys::seL4_WordBits as usize;
    let empty_start = bi.empty.start as usize;
    let empty_end = bi.empty.end as usize;
    let derived_bits = if empty_end > 0 {
        Some(empty_end.next_power_of_two().trailing_zeros() as usize)
    } else {
        None
    };
    let snapshot_bits = BootInfoState::get().map(|state| state.snapshot().init_cnode_bits as usize);

    if init_bits_raw == 0 || init_bits_raw > word_bits {
        let fallback_bits = snapshot_bits
            .filter(|bits| *bits > 0 && *bits <= word_bits)
            .or(derived_bits.filter(|bits| *bits > 0 && *bits <= word_bits))
            .unwrap_or(word_bits);

        log::error!(
            "[sel4.cnode-bits] invalid initBits raw={} word_bits={} empty=[0x{empty_start:04x}..0x{empty_end:04x}) derived_bits={derived_bits:?} snapshot_bits={snapshot_bits:?} fallback={fallback_bits} caller={}:{}",
            init_bits_raw,
            word_bits,
            caller.file(),
            caller.line(),
        );
        return fallback_bits as u8;
    }

    assert!(
        init_bits_raw <= word_bits,
        "initBits must not exceed word width"
    );
    debug_assert!(init_bits_raw > 0, "init CNode capacity must be non-zero");
    init_bits_raw as u8
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapTag {
    Null = 0,
    Frame = 1,
    Untyped = 2,
    PageTable = 3,
    Endpoint = 4,
    Notification = 6,
    Reply = 8,
    VSpace = 9,
    CNode = 10,
    AsidControl = 11,
    Thread = 12,
    AsidPool = 13,
    IrqControl = 14,
    IrqHandler = 16,
    Zombie = 18,
    Domain = 20,
    SgiSignal = 27,
}

impl CapTag {
    #[inline(always)]
    pub const fn from_raw(raw: seL4_Word) -> Option<Self> {
        match raw {
            0 => Some(Self::Null),
            1 => Some(Self::Frame),
            2 => Some(Self::Untyped),
            3 => Some(Self::PageTable),
            4 => Some(Self::Endpoint),
            6 => Some(Self::Notification),
            8 => Some(Self::Reply),
            9 => Some(Self::VSpace),
            10 => Some(Self::CNode),
            11 => Some(Self::AsidControl),
            12 => Some(Self::Thread),
            13 => Some(Self::AsidPool),
            14 => Some(Self::IrqControl),
            16 => Some(Self::IrqHandler),
            18 => Some(Self::Zombie),
            20 => Some(Self::Domain),
            27 => Some(Self::SgiSignal),
            _ => None,
        }
    }

    #[inline(always)]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Frame => "frame",
            Self::Untyped => "untyped",
            Self::PageTable => "page_table",
            Self::Endpoint => "endpoint",
            Self::Notification => "notification",
            Self::Reply => "reply",
            Self::VSpace => "vspace",
            Self::CNode => "cnode",
            Self::AsidControl => "asid_control",
            Self::Thread => "tcb",
            Self::AsidPool => "asid_pool",
            Self::IrqControl => "irq_control",
            Self::IrqHandler => "irq_handler",
            Self::Zombie => "zombie",
            Self::Domain => "domain",
            Self::SgiSignal => "sgi_signal",
        }
    }
}

/// Returns the architectural word width (in bits) exposed by seL4.
#[inline(always)]
pub const fn word_bits() -> seL4_Word {
    WORD_BITS
}

#[inline(always)]
pub const fn cap_data_guard(guard: seL4_Word, guard_size: seL4_Word) -> seL4_Word {
    let guard_masked = guard & 0x3fff_ffff_ffff_ffff;
    let guard_bits = guard_size & 0x3f;
    (guard_masked << 6) | guard_bits
}

use crate::boot::bi_extra::UntypedDesc;
use sel4_sys::{
    seL4_ARM_PageTableObject, seL4_ARM_PageTable_Map, seL4_ARM_Page_Default, seL4_ARM_Page_Map,
    seL4_ARM_VMAttributes, seL4_BootInfo, seL4_SlotRegion, MAX_BOOTINFO_UNTYPEDS,
};

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use sel4_panicking::write_debug_byte;

/// Alias to the boot information structure exposed by `sel4_sys`.
pub type BootInfo = seL4_BootInfo;

#[inline(always)]
pub const fn bootinfo_node_id(bootinfo: &seL4_BootInfo) -> seL4_Word {
    #[cfg(target_os = "none")]
    {
        bootinfo.nodeID
    }
    #[cfg(not(target_os = "none"))]
    {
        bootinfo.nodeId
    }
}

#[inline(always)]
pub const fn vm_attributes_raw(attr: seL4_ARM_VMAttributes) -> seL4_Word {
    #[cfg(target_os = "none")]
    {
        attr
    }
    #[cfg(not(target_os = "none"))]
    {
        attr.0
    }
}

/// Preserves an ARM mapping's cache policy while preventing instruction fetches.
#[inline(always)]
pub const fn vm_attributes_with_execute_never(
    attr: seL4_ARM_VMAttributes,
) -> seL4_ARM_VMAttributes {
    let raw = vm_attributes_raw(attr) | vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever);
    #[cfg(target_os = "none")]
    {
        raw
    }
    #[cfg(not(target_os = "none"))]
    {
        seL4_ARM_VMAttributes(raw)
    }
}

/// Returns the capability pointer for the init thread's root CNode.
#[inline(always)]
pub fn init_cnode_cptr(bi: &seL4_BootInfo) -> seL4_CPtr {
    sel4_view::init_cnode_cptr(bi)
}

/// Guard-encoded node index used by legacy init-CNode addressing probes.
#[inline(always)]
pub fn init_cnode_index_word() -> seL4_Word {
    0
}

/// Returns the radix width (in bits) for the init thread's root CNode.
#[inline(always)]
pub fn init_cnode_bits(bi: &seL4_BootInfo) -> u8 {
    sel4_view::init_cnode_bits(bi)
        .try_into()
        .expect("init CNode bits must fit in u8")
}

/// Returns the `[start, end)` empty slot window advertised by bootinfo.
#[inline(always)]
pub fn empty_window(bi: &seL4_BootInfo) -> (seL4_Word, seL4_Word) {
    sel4_view::empty_window(bi)
}

/// Errors raised while validating a bootinfo pointer and its extra region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootInfoError {
    /// The supplied bootinfo pointer was null.
    Null,
    /// The bootinfo pointer was not aligned to the required boundary.
    Unaligned {
        /// Offending bootinfo pointer address supplied by the caller.
        address: usize,
        /// Alignment (in bytes) required by the `seL4_BootInfo` structure.
        required: usize,
    },
    /// Arithmetic overflow occurred while computing bounds.
    Overflow,
    /// The initThreadCNodeSizeBits field was invalid.
    InitCNodeBits {
        /// Reported radix width in bits.
        bits: usize,
    },
    /// The computed extra range wrapped or was otherwise invalid.
    ExtraRange {
        /// Starting address of the invalid bootinfo extra range.
        start: usize,
        /// End address of the invalid bootinfo extra range.
        end: usize,
        /// The backing limit inferred from bootinfo page counts.
        limit: usize,
    },
}

impl fmt::Display for BootInfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "bootinfo pointer was null"),
            Self::Unaligned { address, required } => {
                write!(
                    f,
                    "bootinfo pointer not {required}-byte aligned: 0x{address:016x}"
                )
            }
            Self::Overflow => write!(f, "bootinfo bounds computation overflowed"),
            Self::InitCNodeBits { bits } => write!(
                f,
                "initThreadCNodeSizeBits out of range: {bits} (expected <= 31)"
            ),
            Self::ExtraRange { start, end, limit } => write!(
                f,
                "bootinfo extra range invalid: [0x{start:016x}..0x{end:016x}) limit=0x{limit:016x}"
            ),
        }
    }
}

fn bootinfo_extra_slice<'a>(
    header: &'a seL4_BootInfo,
    extra_offset: usize,
) -> Result<(&'a [u8], usize, usize, usize), BootInfoError> {
    let addr = header as *const _ as usize;
    let required_align = mem::align_of::<seL4_BootInfo>();
    if required_align != 0 && addr % required_align != 0 {
        return Err(BootInfoError::Unaligned {
            address: addr,
            required: required_align,
        });
    }

    let extra_len = header.extraLen as usize;
    let extra_start = addr
        .checked_add(extra_offset)
        .ok_or(BootInfoError::Overflow)?;
    let extra_end = extra_start
        .checked_add(extra_len)
        .ok_or(BootInfoError::Overflow)?;

    if extra_end < extra_start {
        return Err(BootInfoError::ExtraRange {
            start: extra_start,
            end: extra_end,
            limit: extra_end,
        });
    }

    let page_base = addr & !(IPC_PAGE_BYTES - 1);
    let required_bytes = extra_end
        .checked_sub(page_base)
        .ok_or(BootInfoError::Overflow)?;
    let mapped_bytes = required_bytes.saturating_add(IPC_PAGE_BYTES - 1) & !(IPC_PAGE_BYTES - 1);
    let bootinfo_limit = page_base
        .checked_add(mapped_bytes)
        .ok_or(BootInfoError::Overflow)?;

    // SAFETY: The kernel guarantees that bootinfo and its extra region are mapped as
    // readable memory for the root task. The calculations above ensure we do not
    // wrap the address space or overrun the reported length.
    let slice = unsafe { core::slice::from_raw_parts(extra_start as *const u8, extra_len) };
    Ok((slice, extra_start, extra_end, bootinfo_limit))
}

/// Immutable projection of the kernel-supplied bootinfo region.
#[derive(Clone, Copy)]
pub struct BootInfoView {
    header: &'static seL4_BootInfo,
    extra_bytes: &'static [u8],
    extra_start: usize,
    extra_end: usize,
    extra_limit: usize,
}

// SAFETY: The seL4 bootinfo region is mapped by the kernel for the lifetime of the
// root task. The raw pointers within `seL4_BootInfo` reference kernel-owned memory
// that remains valid and immutable after boot, so sharing `BootInfoView` across
// threads does not introduce additional aliasing or mutation hazards.
unsafe impl Send for BootInfoView {}
unsafe impl Sync for BootInfoView {}

impl BootInfoView {
    fn build(header: &'static seL4_BootInfo) -> Result<Self, BootInfoError> {
        let init_bits = canonical_cnode_bits(header) as usize;
        debug_assert!(
            init_bits <= sel4_sys::seL4_WordBits as usize,
            "initBits must be <= word width",
        );
        if init_bits > sel4_sys::seL4_WordBits as usize {
            ::log::error!("bootinfo initBits invalid: {init_bits} (expected <= seL4_WordBits)");
            return Err(BootInfoError::InitCNodeBits { bits: init_bits });
        }
        let (extra_bytes, extra_start, extra_end, extra_limit) =
            bootinfo_extra_slice(header, BOOTINFO_FRAME_BYTES)?;
        Ok(Self {
            header,
            extra_bytes,
            extra_start,
            extra_end,
            extra_limit,
        })
    }

    /// Constructs a [`BootInfoView`] from a trusted reference.
    pub fn new(header: &'static seL4_BootInfo) -> Result<Self, BootInfoError> {
        Self::build(header)
    }

    /// Constructs a [`BootInfoView`] from a raw pointer after validation.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `ptr` references a live `seL4_BootInfo`
    /// structure for the duration of the returned view.
    pub unsafe fn from_ptr(ptr: *const seL4_BootInfo) -> Result<Self, BootInfoError> {
        let p = NonNull::new(ptr as *mut seL4_BootInfo).ok_or(BootInfoError::Null)?;
        let header = unsafe {
            // SAFETY: `NonNull::new` guarantees the pointer is not null. The
            // caller promises that the pointer references a live
            // `seL4_BootInfo` structure for the required lifetime.
            &*p.as_ptr()
        };
        // The pointer dereference above is safe only if the caller honours the
        // contract documented for this method. All further bounds checks are
        // performed on the resulting reference.
        Self::build(header)
    }

    /// Constructs a [`BootInfoView`] for a snapshotted header using a validated
    /// source view to bound the extra region.
    pub fn from_snapshot_source(
        source: &BootInfoView,
        header: &'static seL4_BootInfo,
    ) -> Result<Self, BootInfoError> {
        let addr = header as *const _ as usize;
        let required_align = mem::align_of::<seL4_BootInfo>();
        if required_align != 0 && addr % required_align != 0 {
            return Err(BootInfoError::Unaligned {
                address: addr,
                required: required_align,
            });
        }

        let extra_len = source.extra().len();
        let snapshot_extra_offset = mem::size_of::<seL4_BootInfo>();
        let extra_start = addr
            .checked_add(snapshot_extra_offset)
            .ok_or(BootInfoError::Overflow)?;
        let extra_end = extra_start
            .checked_add(extra_len)
            .ok_or(BootInfoError::Overflow)?;

        let page_base = addr & !(IPC_PAGE_BYTES - 1);
        let required_bytes = extra_end
            .checked_sub(page_base)
            .ok_or(BootInfoError::Overflow)?;
        let mapped_bytes =
            required_bytes.saturating_add(IPC_PAGE_BYTES - 1) & !(IPC_PAGE_BYTES - 1);
        let slice = unsafe { core::slice::from_raw_parts(extra_start as *const u8, extra_len) };

        Ok(Self {
            header,
            extra_bytes: slice,
            extra_start,
            extra_end,
            extra_limit: extra_start
                .checked_add(mapped_bytes)
                .ok_or(BootInfoError::Overflow)?,
        })
    }

    /// Returns the bootinfo header exposed by this view.
    #[must_use]
    pub fn header(&self) -> &'static seL4_BootInfo {
        self.header
    }

    /// Returns the kernel-advertised extra region as a byte slice.
    #[must_use]
    pub fn extra(&self) -> &'static [u8] {
        self.extra_bytes
    }

    /// Returns the virtual address range containing the bootinfo extra blob.
    #[must_use]
    pub fn extra_range(&self) -> Range<usize> {
        self.extra_start..self.extra_end
    }

    /// Returns the exclusive limit of the mapped bootinfo view.
    #[must_use]
    pub fn extra_limit(&self) -> usize {
        self.extra_limit
    }

    /// Returns the raw bytes that back the bootinfo header.
    #[must_use]
    pub fn header_bytes(&self) -> &'static [u8] {
        let ptr = self.header as *const _ as *const u8;
        // SAFETY: `seL4_BootInfo` is plain data; we rely on the compiler-provided
        // layout and the static lifetime guaranteed by the kernel mapping.
        unsafe { core::slice::from_raw_parts(ptr, mem::size_of::<seL4_BootInfo>()) }
    }

    /// Returns the number of extra words reported by the kernel.
    #[must_use]
    pub fn extra_bytes(&self) -> usize {
        self.header.extraLen as usize
    }

    /// Returns the radix width (in bits) of the init thread's CNode.
    #[must_use]
    pub fn init_cnode_bits(&self) -> u8 {
        canonical_cnode_bits(self.header)
    }

    /// Returns the canonical traversal depth for the init thread CNode.
    #[must_use]
    pub fn init_cnode_depth(&self) -> u8 {
        canonical_cnode_depth(self.init_cnode_bits(), sel4_sys::seL4_WordBits as u8)
    }

    /// Returns the radix width of the init thread's CNode as `usize`.
    #[must_use]
    pub fn init_cnode_size_bits(&self) -> usize {
        usize::from(self.init_cnode_bits())
    }

    /// Returns the inclusive-exclusive slot range advertised as free by the kernel.
    #[must_use]
    pub fn init_cnode_empty_range(&self) -> (seL4_CPtr, seL4_CPtr) {
        (
            self.header.empty.start as seL4_CPtr,
            self.header.empty.end as seL4_CPtr,
        )
    }

    /// Returns the bootinfo-advertised empty slot window as `usize` values.
    #[must_use]
    pub fn init_cnode_empty_usize(&self) -> (usize, usize) {
        (
            self.header.empty.start as usize,
            self.header.empty.end as usize,
        )
    }

    /// Returns the capability designating the init thread's root CNode.
    #[must_use]
    pub fn root_cnode_cap(&self) -> seL4_CPtr {
        sel4_sys::seL4_CapInitThreadCNode
    }

    /// Returns the canonical (guard-less) root CNode capability provided by the kernel.
    ///
    /// This capability can traverse slots below the bootinfo empty window, so it should be used
    /// whenever we need to read kernel-provided caps that live outside the advertised range.
    #[must_use]
    pub fn canonical_root_cap(&self) -> seL4_CPtr {
        canonical_root_cap_ptr()
    }
}

/// Returns the first RAM-backed untyped capability advertised by the kernel.
#[must_use]
pub fn first_regular_untyped(bi: &seL4_BootInfo) -> Option<seL4_CPtr> {
    let count = (bi.untyped.end - bi.untyped.start) as usize;
    let descriptors = &bi.untypedList[..count];
    descriptors.iter().enumerate().find_map(|(index, desc)| {
        if desc.isDevice == 0 {
            Some(bi.untyped.start + index as seL4_CPtr)
        } else {
            None
        }
    })
}

#[cfg(feature = "canonical_cspace")]
#[must_use]
pub fn pick_smallest_non_device_untyped(bi: &seL4_BootInfo) -> seL4_CPtr {
    let count = (bi.untyped.end - bi.untyped.start) as usize;
    let mut best: Option<(u8, seL4_CPtr)> = None;
    for (index, desc) in bi.untypedList[..count].iter().enumerate() {
        if desc.isDevice != 0 {
            continue;
        }
        let cap = bi.untyped.start + index as seL4_CPtr;
        match best {
            Some((bits, _)) if desc.sizeBits as u8 >= bits => {}
            _ => best = Some((desc.sizeBits as u8, cap)),
        }
    }

    match best {
        Some((_, cap)) => cap,
        None => panic!("bootinfo must provide at least one RAM-backed untyped capability"),
    }
}

static ROOT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);
static SEND_LOGGED: AtomicBool = AtomicBool::new(false);

/// Error returned when guarded IPC cannot proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    /// The root endpoint has not been published yet.
    EpNotReady,
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EpNotReady => write!(f, "root endpoint not published"),
        }
    }
}

/// Publish the root endpoint capability once it has been retyped.
#[inline]
pub fn set_ep(ep: seL4_CPtr) {
    lock_ipc_send();
    ROOT_ENDPOINT.store(ep as usize, Ordering::Release);
    if ep == seL4_CapNull {
        SEND_LOGGED.store(false, Ordering::Release);
        set_ep_validated(false);
    }
}

/// Clear the root endpoint pointer. Intended for tests.
#[inline]
pub fn clear_ep() {
    lock_ipc_send();
    ROOT_ENDPOINT.store(0, Ordering::Release);
    set_ep_validated(false);
}

/// Returns the currently published root endpoint capability, if any.
#[inline]
#[must_use]
pub fn root_endpoint() -> seL4_CPtr {
    ROOT_ENDPOINT.load(Ordering::Acquire) as seL4_CPtr
}

/// Returns `true` when the root endpoint has been published.
#[inline]
#[must_use]
pub fn ep_ready() -> bool {
    root_endpoint() != seL4_CapNull
}

#[inline]
pub fn set_ep_validated(validated: bool) {
    EP_VALIDATED.store(validated, Ordering::Release);
}

#[inline]
#[must_use]
pub fn ep_validated() -> bool {
    EP_VALIDATED.load(Ordering::Acquire)
}

#[inline]
pub fn lock_ipc_send() {
    IPC_SEND_UNLOCKED.store(false, Ordering::Release);
}

#[inline]
pub fn unlock_ipc_send() {
    IPC_SEND_UNLOCKED.store(true, Ordering::Release);
}

#[inline]
#[must_use]
pub fn ipc_send_unlocked() -> bool {
    IPC_SEND_UNLOCKED.load(Ordering::Acquire)
}

/// Writes a value into an IPC message register.
#[cfg(feature = "kernel")]
#[inline]
pub fn set_message_register(index: usize, value: seL4_Word) {
    let mr_index: seL4_Word = index
        .try_into()
        .expect("message register index must fit in seL4_Word");
    unsafe { sel4_sys::seL4_SetMR(mr_index, value) };
}

/// Reads a value from an IPC message register.
#[cfg(feature = "kernel")]
#[inline]
pub fn message_register(index: usize) -> seL4_Word {
    let reg_index: seL4_Word = index
        .try_into()
        .expect("message register index must fit in seL4_Word");
    unsafe { sel4_sys::seL4_GetMR(reg_index) }
}

/// Issues a classic seL4 reply using the current thread's implicit reply cap.
///
/// MCS callers must use [`reply_to`] with the explicit Reply object populated
/// by the matching receive operation.
#[cfg(feature = "kernel")]
#[inline]
pub fn reply(info: seL4_MessageInfo) {
    unsafe {
        syscall::reply(info);
    }
}

/// Issues an seL4 reply through an explicit single-use Reply capability.
#[cfg(feature = "kernel")]
#[inline]
pub fn reply_to(reply_cap: seL4_CPtr, info: seL4_MessageInfo, message_registers: [seL4_Word; 4]) {
    unsafe {
        syscall::reply_to(reply_cap, info, &message_registers);
    }
}

#[cfg(feature = "kernel")]
#[track_caller]
#[inline]
pub fn recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { syscall::recv(dest, badge) }
}

/// Receives a call and stores its reply authority in the supplied Reply object.
#[cfg(feature = "kernel")]
#[track_caller]
#[inline]
pub fn recv_with_reply(
    dest: seL4_CPtr,
    badge: *mut seL4_Word,
    reply: seL4_CPtr,
) -> (seL4_MessageInfo, [seL4_Word; 4]) {
    let mut message_registers = [0; 4];
    let info = unsafe { syscall::recv_with_reply(dest, badge, reply, &mut message_registers) };
    (info, message_registers)
}

#[cfg(feature = "kernel")]
#[inline]
pub fn wait(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { syscall::wait(dest, badge) }
}

/// Issues a non-blocking receive on the supplied endpoint.
#[cfg(feature = "kernel")]
#[inline]
pub fn nb_recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { syscall::nb_recv(dest, badge) }
}

/// Nonblockingly receives a call into an explicit Reply object.
#[cfg(feature = "kernel")]
#[inline]
pub fn nb_recv_with_reply(
    dest: seL4_CPtr,
    badge: *mut seL4_Word,
    reply: seL4_CPtr,
) -> seL4_MessageInfo {
    unsafe { syscall::nb_recv_with_reply(dest, badge, reply) }
}

/// Issues a non-blocking wait on the supplied notification object.
#[cfg(feature = "kernel")]
#[inline]
pub fn poll(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { syscall::poll(dest, badge) }
}

/// Yields the current thread to the scheduler.
#[cfg(feature = "kernel")]
#[inline]
pub fn yield_now() {
    unsafe { syscall::yield_now() };
}

/// Issues a raw seL4 send without validating the destination capability.
#[cfg(feature = "kernel")]
#[track_caller]
#[inline(always)]
pub fn send_unchecked(dest: seL4_CPtr, info: seL4_MessageInfo) {
    guard_ipc_destination("send_unchecked", dest);
    unsafe {
        syscall::send(dest, info);
    }
}

/// Issues a raw seL4 non-blocking send without validating the destination capability.
#[cfg(feature = "kernel")]
#[track_caller]
#[inline(always)]
pub fn send_nb_unchecked(dest: seL4_CPtr, info: seL4_MessageInfo) {
    guard_ipc_destination("send_nb_unchecked", dest);
    unsafe {
        syscall::nb_send(dest, info);
    }
}

/// Issues a raw seL4 call without validating the destination capability.
#[cfg(feature = "kernel")]
#[track_caller]
#[inline(always)]
pub fn call_unchecked(dest: seL4_CPtr, info: seL4_MessageInfo) -> seL4_MessageInfo {
    let length = info.length();
    let mut message_registers = [0; 4];
    if length > 0 {
        message_registers[0] = unsafe { sel4_sys::seL4_GetMR(0) };
    }
    if length > 1 {
        message_registers[1] = unsafe { sel4_sys::seL4_GetMR(1) };
    }
    if length > 2 {
        message_registers[2] = unsafe { sel4_sys::seL4_GetMR(2) };
    }
    if length > 3 {
        message_registers[3] = unsafe { sel4_sys::seL4_GetMR(3) };
    }
    let (reply, message_registers) =
        call_with_message_registers_unchecked(dest, info, message_registers);
    // Match libsel4's Call contract: the four fast reply registers are always
    // reflected into the caller's IPC buffer, independently of request length.
    for (index, value) in [0, 1, 2, 3].into_iter().zip(message_registers) {
        unsafe { sel4_sys::seL4_SetMR(index, value) };
    }
    reply
}

/// Issues a raw seL4 call with four caller-owned fast message registers.
///
/// The returned array is the kernel reply state. Keeping a bounded IPC
/// envelope in caller-owned locals avoids an unnecessary IPC-buffer
/// round-trip while preserving the ordinary seL4 Call and Reply semantics.
#[cfg(feature = "kernel")]
#[track_caller]
#[inline(always)]
pub fn call_with_message_registers_unchecked(
    dest: seL4_CPtr,
    info: seL4_MessageInfo,
    message_registers: [seL4_Word; 4],
) -> (seL4_MessageInfo, [seL4_Word; 4]) {
    guard_ipc_destination("call_with_message_registers_unchecked", dest);
    let [mut mr0, mut mr1, mut mr2, mut mr3] = message_registers;
    let reply =
        unsafe { syscall::call_with_mrs(dest, info, &mut mr0, &mut mr1, &mut mr2, &mut mr3) };
    (reply, [mr0, mr1, mr2, mr3])
}

/// Signals a notification capability without validating the destination pointer.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn signal_unchecked(dest: seL4_CPtr) {
    let empty = seL4_MessageInfo::new(0, 0, 0, 0);
    guard_ipc_destination("signal_unchecked", dest);
    unsafe {
        syscall::send(dest, empty);
    }
}

#[cfg(feature = "kernel")]
const IPC_TRAP_LINE_CAP: usize = 240;

#[cfg(feature = "kernel")]
static BOOTSTRAP_SEND_INSTRUMENT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Close the bounded UART IPC trace before temporal child duties are resumed.
///
/// Legal all-ready IPC bypasses bootstrap diagnostics entirely. This explicit
/// boundary also prevents a later readiness regression from emitting the
/// first-three synchronous breadcrumbs on a restricted scheduling context.
#[cfg(feature = "kernel")]
pub fn complete_bootstrap_ipc_trace() {
    BOOTSTRAP_SEND_INSTRUMENT_COUNT.store(3, Ordering::Release);
}

#[cfg(feature = "kernel")]
fn emit_illegal_send_line(line: &str) {
    crate::bootstrap::log::force_uart_line(line);
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug)]
pub enum IpcSyscallKind {
    Send,
    NbSend,
    Call,
    Reply,
    ReplyRecv,
    Recv,
    NbRecv,
    Wait,
}

#[cfg(feature = "kernel")]
#[inline(always)]
fn ipc_bootstrap_trap(kind: IpcSyscallKind, dest: seL4_CPtr, location: &Location) -> bool {
    let ready = ep_ready();
    let validated = ep_validated();
    let unlocked = ipc_send_unlocked();
    let post_commit = boot_log::post_commit_ipc_unlocked();

    // Steady-state IPC must not contend on bootstrap diagnostics. In
    // particular, every restricted MCS duty enters through these wrappers;
    // sharing the trace counter and BootTracer lock after readiness can spend
    // a duty's complete refill before its blocking syscall. Keep the four
    // readiness guards live on every call and isolate all diagnostic work on
    // the pre-readiness path.
    if ready && validated && unlocked && post_commit {
        return false;
    }

    ipc_bootstrap_trap_slow(
        kind,
        dest,
        location,
        ready,
        validated,
        unlocked,
        post_commit,
    )
}

#[cfg(feature = "kernel")]
#[cold]
#[inline(never)]
fn ipc_bootstrap_trap_slow(
    kind: IpcSyscallKind,
    dest: seL4_CPtr,
    location: &Location,
    ready: bool,
    validated: bool,
    unlocked: bool,
    post_commit: bool,
) -> bool {
    let snapshot = crate::bootstrap::boot_tracer().snapshot();

    let trace_count = BOOTSTRAP_SEND_INSTRUMENT_COUNT.fetch_add(1, Ordering::Relaxed);
    if trace_count < 3 {
        let mut trace_line = HeaplessString::<IPC_TRAP_LINE_CAP>::new();
        let _ = write!(
            &mut trace_line,
            "[ipc-trace] kind={kind:?} dest=0x{dest:04x} phase={:?} seq={} ready={} validated={} unlocked={} post_commit={} caller={}:{}",
            snapshot.phase,
            snapshot.sequence,
            ready as u8,
            validated as u8,
            unlocked as u8,
            post_commit as u8,
            location.file(),
            location.line(),
        );
        emit_illegal_send_line(trace_line.as_str());
    }

    let mut info_line = HeaplessString::<IPC_TRAP_LINE_CAP>::new();
    let _ = write!(
        &mut info_line,
        "[ipc-trap] kind={kind:?} cap=0x{dest:04x} phase={:?} seq={} ready={} validated={} unlocked={} post_commit={} caller={}:{}",
        snapshot.phase,
        snapshot.sequence,
        ready as u8,
        validated as u8,
        unlocked as u8,
        post_commit as u8,
        location.file(),
        location.line(),
    );
    emit_illegal_send_line(info_line.as_str());

    true
}

#[inline(never)]
fn ensure_endpoint() -> Result<seL4_CPtr, IpcError> {
    let endpoint = root_endpoint();
    if endpoint == seL4_CapNull {
        serial::puts_once("[ipc] EP not ready; dropping\n");
        Err(IpcError::EpNotReady)
    } else {
        Ok(endpoint)
    }
}

#[inline(never)]
fn guard_ipc_destination(callsite: &str, dest: seL4_CPtr) {
    let ep_slot = root_endpoint();
    let ready = ep_ready();
    let validated = ep_validated();
    let unlocked = ipc_send_unlocked();

    if dest == seL4_CapNull {
        let mut line = HeaplessString::<160>::new();
        let _ = write!(
            line,
            "[ipc-guard] callsite={callsite} cap=0x{cap:04x} ready={ready} validated={validated} unlocked={unlocked}",
            cap = dest,
            ready = ready as u8,
            validated = validated as u8,
            unlocked = unlocked as u8,
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        panic!("[ipc-guard] null capability in {callsite}");
    }

    if dest == ep_slot && (!ready || !validated || !unlocked) {
        let mut line = HeaplessString::<192>::new();
        let _ = write!(
            line,
            "[ipc-guard] blocked callsite={callsite} cap=0x{cap:04x} ep_ready={ready} ep_validated={validated} ipc_unlocked={unlocked}",
            cap = dest,
            ready = ready as u8,
            validated = validated as u8,
            unlocked = unlocked as u8,
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
    }
}

/// Issues an seL4 send only when the endpoint capability is initialised.
#[inline(never)]
pub fn send_guarded(info: seL4_MessageInfo) -> Result<(), IpcError> {
    let endpoint = ensure_endpoint()?;
    debug_assert_ne!(
        endpoint, seL4_CapNull,
        "send_guarded must not transmit on the null endpoint",
    );
    debug_uart_str("[dbg] logger.switch complete; about to send bootstrap to EP 0x0130\n");
    if !SEND_LOGGED.swap(true, Ordering::AcqRel) {
        log::info!("bootstrap: send on ep slot=0x{slot:04x}", slot = endpoint,);
    }
    send_unchecked(endpoint, info);
    debug_uart_str("[dbg] bootstrap send to EP 0x0130 returned\n");
    Ok(())
}

/// Issues an seL4 call only when the endpoint capability is initialised.
#[inline(never)]
pub fn call_guarded(
    info: seL4_MessageInfo,
    mr0: Option<&mut seL4_Word>,
    mr1: Option<&mut seL4_Word>,
    mr2: Option<&mut seL4_Word>,
    mr3: Option<&mut seL4_Word>,
) -> Result<seL4_MessageInfo, IpcError> {
    let endpoint = ensure_endpoint()?;
    guard_ipc_destination("call_guarded", endpoint);
    let m0 = mr0.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let m1 = mr1.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let m2 = mr2.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let m3 = mr3.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let info = unsafe { syscall::call_with_mrs(endpoint, info, m0, m1, m2, m3) };
    Ok(info)
}

/// Issues an seL4 reply+receive cycle only when the endpoint is initialised.
#[inline(never)]
pub fn replyrecv_guarded(
    info: seL4_MessageInfo,
    badge: Option<&mut seL4_Word>,
) -> Result<seL4_MessageInfo, IpcError> {
    let endpoint = ensure_endpoint()?;
    guard_ipc_destination("replyrecv_guarded", endpoint);
    let badge_ptr = badge.map_or(ptr::null_mut(), |b| b as *mut seL4_Word);

    let message = unsafe { syscall::reply_recv(endpoint, info, badge_ptr) };
    Ok(message)
}

/// Issues an explicit-Reply seL4 reply+receive cycle on the root endpoint.
#[inline(never)]
pub fn replyrecv_guarded_with_reply(
    info: seL4_MessageInfo,
    badge: Option<&mut seL4_Word>,
    reply: seL4_CPtr,
) -> Result<seL4_MessageInfo, IpcError> {
    let endpoint = ensure_endpoint()?;
    guard_ipc_destination("replyrecv_guarded_with_reply", endpoint);
    let badge_ptr = badge.map_or(ptr::null_mut(), |b| b as *mut seL4_Word);

    let message = unsafe { syscall::reply_recv_with_reply(endpoint, info, badge_ptr, reply) };
    Ok(message)
}

/// Returns the traversal depth (in bits) for init CNode syscall invocations.
#[inline]
pub fn init_cnode_depth(_bi: &seL4_BootInfo) -> u8 {
    let init_bits = canonical_cnode_bits(_bi);
    canonical_cnode_depth(init_bits, WORD_BITS as u8)
}

/// Emits a single byte to the seL4 debug console.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn debug_put_char(ch: i32) {
    debug_put_char_raw(ch as u8);
}

/// Emits a byte to the seL4 debug console using the raw debug syscall.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn debug_put_char_raw(byte: u8) {
    debug_put_bytes_raw(core::slice::from_ref(&byte));
}

/// Emits a byte slice to the seL4 debug console without taking the UART TX lock.
#[cfg(feature = "kernel")]
#[inline(always)]
pub(crate) fn debug_put_bytes_unlocked(bytes: &[u8]) {
    if serial::serial_root_uart_released_for_linked_runtime() {
        // The linked runtime may already be executing INIT even though its
        // root client is not active yet. Low-level debug callers have no
        // retained ticket, so fail closed instead of racing UART MMIO.
        return;
    }
    for &byte in bytes {
        #[cfg(sel4_config_printing)]
        {
            // SAFETY: seL4 exposes DebugPutChar as a side-effect-only
            // diagnostic syscall and accepts any byte value.
            unsafe { seL4_DebugPutChar(byte) }
        }
        #[cfg(not(sel4_config_printing))]
        {
            // Production kernels omit DebugPutChar. Route diagnostics through
            // the validated user-provided sink once UART MMIO is admitted.
            write_debug_byte(byte);
        }
    }
}

/// Emits a line to the seL4 debug console without taking the UART TX lock.
#[cfg(feature = "kernel")]
#[inline(always)]
pub(crate) fn debug_put_line_unlocked(line: &[u8]) {
    debug_put_bytes_unlocked(line);
    debug_put_bytes_unlocked(b"\r\n");
}

/// Emits a byte slice to the seL4 debug console using the raw debug syscall.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn debug_put_bytes_raw(bytes: &[u8]) {
    serial::with_uart_tx_lock(|| debug_put_bytes_unlocked(bytes));
}

/// Emits a line (with CRLF) to the seL4 debug console using the raw debug syscall.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn debug_put_line_raw(line: &[u8]) {
    serial::with_uart_tx_lock(|| debug_put_line_unlocked(line));
}

#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub fn debug_put_char(_ch: i32) {}

#[cfg(all(test, not(feature = "kernel")))]
const DEBUG_UART_CAPTURE_LEN: usize = 512;

#[cfg(all(test, not(feature = "kernel")))]
static DEBUG_UART_CAPTURE: SpinMutex<HeaplessVec<u8, DEBUG_UART_CAPTURE_LEN>> =
    SpinMutex::new(HeaplessVec::new());

/// Emits a byte to the debug UART in host builds without touching MMIO.
#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub fn debug_put_char_raw(byte: u8) {
    #[cfg(test)]
    {
        let mut guard = DEBUG_UART_CAPTURE.lock();
        let _ = guard.push(byte);
        return;
    }

    let _ = byte;
}

/// Emits a byte slice to the debug UART in host builds without touching MMIO.
#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub fn debug_put_bytes_raw(bytes: &[u8]) {
    for &byte in bytes {
        debug_put_char_raw(byte);
    }
}

/// Emits a line (with CRLF) to the debug UART in host builds without touching MMIO.
#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub fn debug_put_line_raw(line: &[u8]) {
    debug_put_bytes_raw(line);
    debug_put_bytes_raw(b"\r\n");
}

/// Emits bytes without an extra UART lock in host builds.
#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub(crate) fn debug_put_bytes_unlocked(bytes: &[u8]) {
    debug_put_bytes_raw(bytes);
}

/// Emits a line without an extra UART lock in host builds.
#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub(crate) fn debug_put_line_unlocked(line: &[u8]) {
    debug_put_line_raw(line);
}

/// Clears the captured UART buffer in host tests.
#[cfg(all(test, not(feature = "kernel")))]
pub fn clear_debug_uart_capture() {
    let mut guard = DEBUG_UART_CAPTURE.lock();
    guard.clear();
}

/// Returns the captured UART bytes emitted during a host test.
#[cfg(all(test, not(feature = "kernel")))]
pub fn take_debug_uart_capture() -> HeaplessVec<u8, DEBUG_UART_CAPTURE_LEN> {
    let mut guard = DEBUG_UART_CAPTURE.lock();
    let mut out = HeaplessVec::new();
    core::mem::swap(&mut *guard, &mut out);
    out
}

#[cfg(all(feature = "kernel", target_arch = "aarch64", sel4_config_printing))]
#[no_mangle]
/// Executes the `DebugPutChar` seL4 syscall to emit a byte on the debug console.
pub unsafe extern "C" fn seL4_DebugPutChar(byte: u8) {
    sel4_sys::debug_put_char(byte);
}

#[cfg(all(feature = "kernel", target_arch = "aarch64", not(sel4_config_printing)))]
#[no_mangle]
/// Stub used when kernel printing is disabled for the active seL4 configuration.
pub unsafe extern "C" fn seL4_DebugPutChar(_byte: u8) {}

#[cfg(all(feature = "kernel", target_arch = "aarch64", sel4_config_debug_build))]
#[inline(always)]
/// Requests the kernel to halt execution of the current thread via the debug syscall.
pub fn debug_halt() {
    sel4_sys::debug_halt();
}

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", sel4_config_debug_build)))]
#[inline(always)]
/// Stub used when the kernel omits the debug halt syscall.
pub fn debug_halt() {}

#[cfg(all(feature = "kernel", target_arch = "aarch64", sel4_config_debug_build))]
#[inline(always)]
/// Executes the `DebugCapIdentify` seL4 syscall to reveal a capability's kernel tag.
pub unsafe fn seL4_DebugCapIdentify(slot: seL4_CPtr) -> seL4_Word {
    unsafe { sel4_sys::seL4_DebugCapIdentify(slot) }
}

#[cfg(all(feature = "kernel", not(target_arch = "aarch64")))]
#[no_mangle]
/// Fallback stub for architectures without a debug console syscall implementation.
pub unsafe extern "C" fn seL4_DebugPutChar(_byte: u8) {}

#[cfg(all(feature = "kernel", sel4_config_debug_build))]
/// Requests the kernel to reveal the capability type stored at the provided slot index.
#[inline(always)]
pub fn debug_cap_identify(slot: seL4_CPtr) -> seL4_Word {
    unsafe { sel4_sys::seL4_DebugCapIdentify(slot) as seL4_Word }
}

#[cfg(all(feature = "kernel", not(sel4_config_debug_build)))]
/// Returns zero because the kernel configuration omits the debug capability identification syscall.
#[inline(always)]
pub fn debug_cap_identify(_slot: seL4_CPtr) -> seL4_Word {
    0
}

/// Reports whether the selected kernel exposes capability identification.
///
/// A successful production invocation, such as `Untyped_Retype`, remains the
/// authority for the capability it creates. `DebugCapIdentify` is optional
/// diagnostic evidence and must never become a production boot dependency.
#[inline(always)]
pub const fn debug_cap_identify_available() -> bool {
    cfg!(all(feature = "kernel", sel4_config_debug_build))
}

#[cfg(not(feature = "kernel"))]
/// Returns zero because the function executes only when building for the host.
#[inline(always)]
pub fn debug_cap_identify(_slot: seL4_CPtr) -> seL4_Word {
    0
}

/// Returns the physical address for a mapped ARM page capability.
#[cfg(all(feature = "kernel", target_os = "none"))]
pub fn page_get_address(frame: seL4_CPtr) -> Result<usize, seL4_Error> {
    let mut mr0: seL4_Word = 0;
    let mut mr1: seL4_Word = 0;
    let mut mr2: seL4_Word = 0;
    let mut mr3: seL4_Word = 0;
    let tag = sel4_sys::seL4_MessageInfo::new(
        sel4_sys::arch_invocation_label_ARMPageGetAddress as seL4_Word,
        0,
        0,
        0,
    );
    let output =
        unsafe { sel4_sys::seL4_CallWithMRs(frame, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3) };
    let err = sel4_sys::seL4_MessageInfo_get_label(output) as seL4_Error;
    if err == seL4_NoError {
        Ok(mr0 as usize)
    } else {
        Err(err)
    }
}

#[cfg(all(feature = "kernel", not(target_os = "none")))]
/// Host-test stub used when page address queries are unavailable.
pub fn page_get_address(_frame: seL4_CPtr) -> Result<usize, seL4_Error> {
    Err(sel4_sys::seL4_IllegalOperation)
}

/// Assigns an ASID from an existing ASID pool to a newly created VSpace root.
#[cfg(feature = "kernel")]
pub fn assign_vspace_asid(asid_pool: seL4_CPtr, vspace: seL4_CPtr) -> Result<(), seL4_Error> {
    // SAFETY: The caller supplies kernel capabilities naming an ASID pool and a
    // VSpace cap. seL4 validates both caps and whether the VSpace already has an ASID.
    let err = unsafe { sel4_sys::seL4_ARM_ASIDPool_Assign(asid_pool, vspace) };
    if err == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[vspace] asid-assign failed pool=0x{asid_pool:04x} vspace=0x{vspace:04x} err={err} ({name})",
            name = error_name(err),
        );
        Err(err)
    }
}

/// Maps a page capability into an explicitly supplied VSpace.
#[cfg(feature = "kernel")]
pub fn map_page_into_vspace(
    frame_cap: seL4_CPtr,
    vspace: seL4_CPtr,
    vaddr: usize,
    rights: sel4_sys::seL4_CapRights,
    attr: sel4_sys::seL4_ARM_VMAttributes,
) -> Result<(), seL4_Error> {
    KernelEnv::assert_page_aligned(vaddr);
    let vaddr_word =
        sel4_sys::seL4_Word::try_from(vaddr).expect("virtual address must fit in seL4_Word");
    let result = map_page_into_vspace_syscall(frame_cap, vspace, vaddr_word, rights, attr);
    if let Err(err) = result {
        ::log::error!(
            "[vspace] page-map failed frame=0x{frame_cap:04x} vspace=0x{vspace:04x} vaddr=0x{vaddr:016x} err={err} ({name})",
            name = error_name(err),
        );
    }
    result
}

/// Map one page without diagnostic formatting or logger acquisition.
/// The containment caller supplies the retained page-aligned root mapping
/// address constructed and validated during NineDoor service admission.
#[cfg(feature = "kernel")]
pub(crate) fn map_page_into_vspace_bounded(
    frame_cap: seL4_CPtr,
    vspace: seL4_CPtr,
    vaddr: usize,
    rights: sel4_sys::seL4_CapRights,
    attr: sel4_sys::seL4_ARM_VMAttributes,
) -> Result<(), seL4_Error> {
    map_page_into_vspace_syscall(
        frame_cap,
        vspace,
        vaddr as sel4_sys::seL4_Word,
        rights,
        attr,
    )
}

#[cfg(feature = "kernel")]
#[inline(always)]
fn map_page_into_vspace_syscall(
    frame_cap: seL4_CPtr,
    vspace: seL4_CPtr,
    vaddr_word: sel4_sys::seL4_Word,
    rights: sel4_sys::seL4_CapRights,
    attr: sel4_sys::seL4_ARM_VMAttributes,
) -> Result<(), seL4_Error> {
    // SAFETY: `frame_cap` names a page capability, `vspace` names the target
    // VSpace, and `vaddr_word` is page-aligned. seL4 validates page-table presence,
    // rights, attributes, and authority.
    let err = unsafe { sel4_sys::seL4_ARM_Page_Map(frame_cap, vspace, vaddr_word, rights, attr) };
    if err == seL4_NoError {
        Ok(())
    } else {
        Err(err)
    }
}

/// Maps a page-table capability into an explicitly supplied VSpace.
#[cfg(feature = "kernel")]
pub fn map_page_table_into_vspace(
    page_table: seL4_CPtr,
    vspace: seL4_CPtr,
    vaddr: usize,
    attr: sel4_sys::seL4_ARM_VMAttributes,
) -> Result<(), seL4_Error> {
    KernelEnv::assert_page_aligned(vaddr);
    let vaddr_word = sel4_sys::seL4_Word::try_from(vaddr)
        .expect("page-table virtual address must fit in seL4_Word");
    // SAFETY: `page_table` and `vspace` are caller-supplied kernel caps, and
    // seL4 validates that the table can be installed at the requested address.
    let err = unsafe { sel4_sys::seL4_ARM_PageTable_Map(page_table, vspace, vaddr_word, attr) };
    if err == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[vspace] page-table-map failed table=0x{page_table:04x} vspace=0x{vspace:04x} vaddr=0x{vaddr:016x} err={err} ({name})",
            name = error_name(err),
        );
        Err(err)
    }
}

#[cfg(not(feature = "kernel"))]
/// Host stub used when page address queries require the kernel feature.
pub fn page_get_address(_frame: seL4_CPtr) -> Result<usize, seL4_Error> {
    Err(sel4_sys::seL4_IllegalOperation)
}

#[cfg(all(feature = "kernel", target_os = "none"))]
static USER_IMAGE_PADDR_RANGE: SpinOnce<Option<Range<usize>>> = SpinOnce::new();

/// Returns the page-aligned base virtual address of the root-task user image.
#[cfg(all(feature = "kernel", target_os = "none"))]
#[must_use]
pub fn user_image_base_vaddr() -> usize {
    extern "C" {
        static __text_start: u8;
    }
    let base = core::ptr::addr_of!(__text_start) as usize;
    base & !(PAGE_SIZE - 1)
}

#[cfg(any(not(feature = "kernel"), not(target_os = "none")))]
#[must_use]
pub fn user_image_base_vaddr() -> usize {
    0
}

/// Resolves a user-image virtual address to its physical address when available.
#[cfg(all(feature = "kernel", target_os = "none"))]
#[must_use]
pub fn user_image_vaddr_to_paddr(vaddr: usize) -> Option<usize> {
    let bootinfo = runtime_bootinfo();
    let base = user_image_base_vaddr();
    if vaddr < base {
        return None;
    }
    let offset = vaddr - base;
    let index = offset >> PAGE_BITS;
    let start = bootinfo.userImageFrames.start as usize;
    let end = bootinfo.userImageFrames.end as usize;
    let slot = start.saturating_add(index);
    if slot >= end {
        return None;
    }
    let page_paddr = page_get_address(slot as seL4_CPtr).ok()?;
    Some(page_paddr.saturating_add(offset & (PAGE_SIZE - 1)))
}

/// Returns the boot-provided user-image frame cap covering a virtual address.
#[cfg(all(feature = "kernel", target_os = "none"))]
#[must_use]
pub fn user_image_frame_cap_for_vaddr(vaddr: usize) -> Option<seL4_CPtr> {
    let bootinfo = runtime_bootinfo();
    let base = user_image_base_vaddr();
    if vaddr < base {
        return None;
    }
    let offset = vaddr - base;
    let index = offset >> PAGE_BITS;
    let start = bootinfo.userImageFrames.start as usize;
    let end = bootinfo.userImageFrames.end as usize;
    let slot = start.saturating_add(index);
    (slot < end).then_some(slot as seL4_CPtr)
}

#[cfg(any(not(feature = "kernel"), not(target_os = "none")))]
#[must_use]
pub fn user_image_vaddr_to_paddr(_vaddr: usize) -> Option<usize> {
    None
}

/// Host-build placeholder for user-image frame cap resolution.
#[cfg(any(not(feature = "kernel"), not(target_os = "none")))]
#[must_use]
pub fn user_image_frame_cap_for_vaddr(_vaddr: usize) -> Option<seL4_CPtr> {
    None
}

/// Returns the physical address range covering the root-task user image frames.
#[cfg(all(feature = "kernel", target_os = "none"))]
#[must_use]
pub fn user_image_paddr_range() -> Option<Range<usize>> {
    let cached = USER_IMAGE_PADDR_RANGE.call_once(|| {
        let bootinfo = runtime_bootinfo();
        let start = bootinfo.userImageFrames.start as usize;
        let end = bootinfo.userImageFrames.end as usize;
        if start >= end {
            return None;
        }
        let first = page_get_address(start as seL4_CPtr).ok()?;
        let last = page_get_address((end - 1) as seL4_CPtr).ok()?;
        Some(first..last.saturating_add(PAGE_SIZE))
    });
    cached.clone()
}

#[cfg(any(not(feature = "kernel"), not(target_os = "none")))]
#[must_use]
pub fn user_image_paddr_range() -> Option<Range<usize>> {
    None
}

#[cfg(feature = "kernel")]
fn reserve_bootinfo_snapshot_paddrs(untyped: &mut UntypedCatalog<'_>) {
    let Some(state) = BootInfoState::get() else {
        boot_log::force_uart_line("[bootinfo:snapshot] reserve skipped: state unavailable");
        log::warn!("[bootinfo:snapshot] reserve skipped: state unavailable");
        return;
    };

    let region = state.snapshot_region();
    let page_start = region.start & !(PAGE_SIZE - 1);
    let page_end = region.end.saturating_add(PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let mut begin_line = HeaplessString::<176>::new();
    let _ = write!(
        &mut begin_line,
        "[bootinfo:snapshot] reserve begin vaddr=[0x{start:016x}..0x{end:016x})",
        start = page_start,
        end = page_end,
    );
    boot_log::force_uart_line(begin_line.as_str());
    log::info!("{}", begin_line.as_str());
    if page_start >= page_end {
        return;
    }

    let mut total_pages = 0usize;
    let mut resolved_pages = 0usize;
    let mut segments = 0usize;
    let mut missing_pages = false;
    let mut seg_start: Option<usize> = None;
    let mut seg_end = 0usize;
    let mut first_paddr: Option<usize> = None;
    let mut last_paddr_end = 0usize;

    for vaddr in (page_start..page_end).step_by(PAGE_SIZE) {
        total_pages = total_pages.saturating_add(1);
        let Some(paddr) = user_image_vaddr_to_paddr(vaddr) else {
            missing_pages = true;
            if let Some(start) = seg_start.take() {
                untyped.reserve_paddr_range(start..seg_end, "bootinfo-snapshot");
                segments = segments.saturating_add(1);
                if first_paddr.is_none() {
                    first_paddr = Some(start);
                }
                last_paddr_end = seg_end;
                seg_end = 0;
            }
            continue;
        };

        resolved_pages = resolved_pages.saturating_add(1);
        match seg_start {
            Some(start) if paddr == seg_end => {
                seg_end = paddr.saturating_add(PAGE_SIZE);
                debug_assert!(seg_end >= paddr, "bootinfo snapshot paddr overflow");
                if first_paddr.is_none() {
                    first_paddr = Some(start);
                }
                last_paddr_end = seg_end;
            }
            Some(start) => {
                untyped.reserve_paddr_range(start..seg_end, "bootinfo-snapshot");
                segments = segments.saturating_add(1);
                if first_paddr.is_none() {
                    first_paddr = Some(start);
                }
                last_paddr_end = seg_end;
                seg_start = Some(paddr);
                seg_end = paddr.saturating_add(PAGE_SIZE);
            }
            None => {
                seg_start = Some(paddr);
                seg_end = paddr.saturating_add(PAGE_SIZE);
            }
        }
    }

    if let Some(start) = seg_start.take() {
        untyped.reserve_paddr_range(start..seg_end, "bootinfo-snapshot");
        segments = segments.saturating_add(1);
        if first_paddr.is_none() {
            first_paddr = Some(start);
        }
        last_paddr_end = seg_end;
    }

    let mut line = HeaplessString::<256>::new();
    match first_paddr {
        Some(first) => {
            let _ = write!(
                &mut line,
                "[bootinfo:snapshot] reserved paddr pages={resolved}/{total} segments={segments} missing={missing} vaddr=[0x{vstart:016x}..0x{vend:016x}) paddr=[0x{pstart:016x}..0x{pend:016x})",
                resolved = resolved_pages,
                total = total_pages,
                missing = missing_pages as u8,
                vstart = page_start,
                vend = page_end,
                pstart = first,
                pend = last_paddr_end,
            );
            boot_log::force_uart_line(line.as_str());
            log::info!("{}", line.as_str());
        }
        None => {
            let _ = write!(
                &mut line,
                "[bootinfo:snapshot] failed to resolve paddr pages={total} vaddr=[0x{vstart:016x}..0x{vend:016x})",
                total = total_pages,
                vstart = page_start,
                vend = page_end,
            );
            boot_log::force_uart_line(line.as_str());
            log::warn!("{}", line.as_str());
        }
    }
}

/// Sets the CPU affinity for a TCB when SMP is enabled.
#[cfg(feature = "kernel")]
fn set_tcb_affinity_impl(
    tcb_cap: seL4_CPtr,
    core: u8,
    emit_guard_breadcrumb: bool,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.SetAffinity";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    if emit_guard_breadcrumb {
        let mut breadcrumb = HeaplessString::<96>::new();
        let _ = fmt::write(
            &mut breadcrumb,
            format_args!("tcb=0x{tcb:04x} core={core}", tcb = guarded_tcb),
        );
        sel4_guard::uart_breadcrumb(guard_stage, "seL4_TCB_SetAffinity", breadcrumb.as_str());
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    let result = unsafe { sel4_sys::seL4_TCB_SetAffinity(guarded_tcb, core as seL4_Word) };
    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    let result = {
        let _ = (guarded_tcb, core);
        // MCS core placement is established by the per-core SchedControl cap
        // used to configure the TCB's scheduling context. SetAffinity is not
        // part of the MCS object ABI.
        sel4_sys::seL4_IllegalOperation
    };
    #[cfg(not(target_os = "none"))]
    let result = {
        let _ = (guarded_tcb, core);
        seL4_NoError
    };

    if result == seL4_NoError {
        if emit_guard_breadcrumb {
            let mut breadcrumb = HeaplessString::<96>::new();
            let _ = fmt::write(
                &mut breadcrumb,
                format_args!("tcb=0x{tcb:04x} core={core} result=ok", tcb = guarded_tcb),
            );
            sel4_guard::uart_breadcrumb(
                guard_stage,
                "seL4_TCB_SetAffinity.return",
                breadcrumb.as_str(),
            );
        }
        Ok(())
    } else {
        ::log::error!(
            "[tcb] affinity failed tcb=0x{tcb:04x} core={core} err={err} ({name})",
            tcb = tcb_cap,
            core = core,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Sets the CPU affinity for a TCB and emits a guard breadcrumb on UART.
#[cfg(feature = "kernel")]
pub fn set_tcb_affinity(tcb_cap: seL4_CPtr, core: u8) -> Result<(), seL4_Error> {
    set_tcb_affinity_impl(tcb_cap, core, true)
}

/// Sets the CPU affinity for a TCB without emitting a guard breadcrumb.
///
/// Use this when the caller already emits its own mirrored operator-facing lines
/// and extra UART-only breadcrumbs would create output skew across consoles.
#[cfg(feature = "kernel")]
pub fn set_tcb_affinity_silent(tcb_cap: seL4_CPtr, core: u8) -> Result<(), seL4_Error> {
    set_tcb_affinity_impl(tcb_cap, core, false)
}

/// Sets a TCB priority through the configured seL4 kernel invocation shape.
#[cfg(feature = "kernel")]
pub fn set_tcb_priority(
    tcb_cap: seL4_CPtr,
    authority_tcb: seL4_CPtr,
    priority: u8,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.SetPriority";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let guarded_authority = sel4_guard::guard_cptr(guard_stage, "authority_tcb", authority_tcb);
    // SAFETY: The guarded CPtrs are kernel capabilities supplied by bootstrap code; seL4
    // validates authority and priority bounds.
    let result = unsafe {
        sel4_sys::seL4_TCB_SetPriority(guarded_tcb, guarded_authority, priority as seL4_Word)
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] set-priority failed tcb=0x{tcb:04x} authority=0x{authority:04x} priority={priority} err={err} ({name})",
            tcb = guarded_tcb,
            authority = guarded_authority,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Sets classic TCB scheduling parameters through the seL4 kernel invocation shape.
///
/// MCS callers must use [`set_tcb_sched_params_mcs`] so the scheduling context
/// and fault endpoint cannot be omitted.
#[cfg(feature = "kernel")]
pub fn set_tcb_sched_params(
    tcb_cap: seL4_CPtr,
    authority_tcb: seL4_CPtr,
    mcp: u8,
    priority: u8,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.SetSchedParams";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let guarded_authority = sel4_guard::guard_cptr(guard_stage, "authority_tcb", authority_tcb);
    #[cfg(not(sel4_config_kernel_mcs))]
    // SAFETY: The guarded CPtrs are kernel capabilities supplied by bootstrap code; seL4
    // validates authority and scheduler bounds for the configured kernel.
    let result = unsafe {
        sel4_sys::seL4_TCB_SetSchedParams(
            guarded_tcb,
            guarded_authority,
            mcp as seL4_Word,
            priority as seL4_Word,
        )
    };
    #[cfg(sel4_config_kernel_mcs)]
    let result = {
        let _ = (guarded_tcb, guarded_authority, mcp, priority);
        sel4_sys::seL4_IllegalOperation
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] set-sched-params failed tcb=0x{tcb:04x} authority=0x{authority:04x} mcp={mcp} priority={priority} err={err} ({name})",
            tcb = guarded_tcb,
            authority = guarded_authority,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Attaches an MCS scheduling context and fault endpoint while setting priority.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn set_tcb_sched_params_mcs(
    tcb_cap: seL4_CPtr,
    authority_tcb: seL4_CPtr,
    mcp: u8,
    priority: u8,
    sched_context: seL4_CPtr,
    fault_ep: seL4_CPtr,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.SetSchedParamsMCS";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let guarded_authority = sel4_guard::guard_cptr(guard_stage, "authority_tcb", authority_tcb);
    let guarded_sched_context = sel4_guard::guard_cptr(guard_stage, "sched_context", sched_context);
    // SAFETY: Every CPtr is supplied by bootstrap capability allocation. The
    // kernel validates authority, SC association, fault endpoint rights, and
    // scheduler bounds before changing the TCB.
    let result = unsafe {
        sel4_sys::seL4_TCB_SetSchedParamsMcs(
            guarded_tcb,
            guarded_authority,
            mcp as seL4_Word,
            priority as seL4_Word,
            guarded_sched_context,
            fault_ep,
        )
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] MCS set-sched-params failed tcb=0x{tcb:04x} authority=0x{authority:04x} sc=0x{sc:04x} fault_ep=0x{fault_ep:04x} mcp={mcp} priority={priority} err={err} ({name})",
            tcb = guarded_tcb,
            authority = guarded_authority,
            sc = guarded_sched_context,
            fault_ep = fault_ep,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Configure a passive MCS server with no bound scheduling context.
///
/// The null SC is intentional and compiler-validated: the server may execute
/// only while an allowlisted synchronous caller donates its SC. The TCB still
/// receives an explicit fault endpoint and bounded priority/MCP.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn set_passive_tcb_sched_params_mcs(
    tcb_cap: seL4_CPtr,
    authority_tcb: seL4_CPtr,
    mcp: u8,
    priority: u8,
    fault_ep: seL4_CPtr,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.SetPassiveSchedParamsMCS";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let guarded_authority = sel4_guard::guard_cptr(guard_stage, "authority_tcb", authority_tcb);
    let guarded_fault = sel4_guard::guard_cptr(guard_stage, "fault_ep", fault_ep);
    // SAFETY: The TCB, authority, and fault endpoint come from generated HAL
    // construction. A null SC is the seL4 MCS passive-server contract and is
    // not dereferenced; the kernel validates all remaining scheduler bounds.
    let result = unsafe {
        sel4_sys::seL4_TCB_SetSchedParamsMcs(
            guarded_tcb,
            guarded_authority,
            mcp as seL4_Word,
            priority as seL4_Word,
            sel4_sys::seL4_CapNull,
            guarded_fault,
        )
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] MCS passive set-sched-params failed tcb=0x{tcb:04x} authority=0x{authority:04x} fault_ep=0x{fault_ep:04x} mcp={mcp} priority={priority} err={err} ({name})",
            tcb = guarded_tcb,
            authority = guarded_authority,
            fault_ep = guarded_fault,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Installs the timeout-fault endpoint for an MCS TCB.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn set_tcb_timeout_endpoint(
    tcb_cap: seL4_CPtr,
    timeout_fault_ep: seL4_CPtr,
) -> Result<(), seL4_Error> {
    // SAFETY: The TCB and endpoint CPtrs come from bootstrap allocation. seL4
    // validates the endpoint type and rights before installation.
    let result = unsafe { sel4_sys::seL4_TCB_SetTimeoutEndpoint(tcb_cap, timeout_fault_ep) };
    if result == seL4_NoError {
        Ok(())
    } else {
        Err(result)
    }
}

/// Configures an MCS scheduling context on the core owning `sched_control`.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn configure_sched_context(
    sched_control: seL4_CPtr,
    sched_context: seL4_CPtr,
    budget_us: u64,
    period_us: u64,
    extra_refills: seL4_Word,
    badge: seL4_Word,
    flags: seL4_Word,
) -> Result<(), seL4_Error> {
    // SAFETY: The SchedControl and SC CPtrs are selected from generated
    // BootInfo/untyped authority. seL4 validates budget, period, refill, and
    // association constraints.
    let result = unsafe {
        sel4_sys::seL4_SchedControl_ConfigureFlags(
            sched_control,
            sched_context,
            budget_us,
            period_us,
            extra_refills,
            badge,
            flags,
        )
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        Err(result)
    }
}

/// Returns and resets the SC's consumed-time evidence.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn sched_context_consumed(sched_context: seL4_CPtr) -> Result<u64, seL4_Error> {
    // SAFETY: The SC CPtr is owned by root bootstrap and the kernel validates
    // the invoked object type.
    let result = unsafe { sel4_sys::seL4_SchedContext_Consumed(sched_context) };
    let error = result.error as seL4_Error;
    if error == seL4_NoError {
        Ok(result.consumed)
    } else {
        Err(error)
    }
}

/// Yields to the TCB bound to an MCS scheduling context.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn yield_to_sched_context(sched_context: seL4_CPtr) -> Result<u64, seL4_Error> {
    // SAFETY: The SC CPtr is owned by root bootstrap and the kernel validates
    // that the object is bound to a runnable TCB.
    let result = unsafe { sel4_sys::seL4_SchedContext_YieldTo(sched_context) };
    let error = result.error as seL4_Error;
    if error == seL4_NoError {
        Ok(result.consumed)
    } else {
        Err(error)
    }
}

/// Unbinds one exact TCB from its MCS scheduling context before generation revoke.
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
pub fn unbind_sched_context_object(
    sched_context: seL4_CPtr,
    tcb_cap: seL4_CPtr,
    current_ipc_buffer: Option<usize>,
) -> Result<(), seL4_Error> {
    let current_ipc_buffer = match current_ipc_buffer {
        Some(address)
            if address == 0 || address % core::mem::align_of::<sel4_sys::seL4_IPCBuffer>() != 0 =>
        {
            return Err(sel4_sys::seL4_InvalidArgument)
        }
        value => value,
    };
    let ipc_buffer = current_ipc_buffer.map_or(core::ptr::null_mut(), |address| {
        address as *mut sel4_sys::seL4_IPCBuffer
    });
    // SAFETY: Both CPtrs are retained for the same constructed generation.
    // The supplied IPC-buffer mapping is either null for the init TCB or the
    // exact buffer bound to the restricted critical TCB and retained by its
    // `CriticalChildBacking`. The syscall wrapper writes only that buffer's
    // extra-cap lane, and seL4 validates the object type and association.
    let result =
        unsafe { sel4_sys::seL4_SchedContext_UnbindObject(sched_context, tcb_cap, ipc_buffer) };
    if result == seL4_NoError {
        Ok(())
    } else {
        Err(result)
    }
}

/// Sets the CSpace/VSpace/fault endpoint for a TCB.
#[cfg(feature = "kernel")]
pub fn set_tcb_space(
    tcb_cap: seL4_CPtr,
    fault_ep: seL4_CPtr,
    cspace_root: seL4_CNode,
    cspace_root_data: seL4_Word,
    vspace_root: seL4_CPtr,
    vspace_root_data: seL4_Word,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.SetSpace";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let guarded_cspace = sel4_guard::guard_cptr(guard_stage, "cspace_root", cspace_root);
    let guarded_vspace = sel4_guard::guard_cptr(guard_stage, "vspace_root", vspace_root);
    // SAFETY: The guarded CPtrs are kernel capabilities supplied by bootstrap code. seL4
    // validates the target TCB, CSpace root, VSpace root, and target fault endpoint.
    let result = unsafe {
        sel4_sys::seL4_TCB_SetSpace(
            guarded_tcb,
            fault_ep,
            guarded_cspace,
            cspace_root_data,
            guarded_vspace,
            vspace_root_data,
        )
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] set-space failed tcb=0x{tcb:04x} fault_ep=0x{fault_ep:04x} cspace=0x{cspace:04x} vspace=0x{vspace:04x} err={err} ({name})",
            tcb = guarded_tcb,
            cspace = guarded_cspace,
            vspace = guarded_vspace,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Writes the initial AArch64 register set for a newly configured TCB.
#[cfg(feature = "kernel")]
pub fn write_tcb_registers(
    tcb_cap: seL4_CPtr,
    entry: usize,
    stack_top: usize,
    arg0: seL4_Word,
    resume_target: bool,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.WriteRegisters";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let mut breadcrumb = HeaplessString::<128>::new();
    let _ = fmt::write(
        &mut breadcrumb,
        format_args!(
            "tcb=0x{tcb:04x} entry=0x{entry:016x} sp=0x{sp:016x} arg0=0x{arg0:x} resume={resume}",
            tcb = guarded_tcb,
            entry = entry,
            sp = stack_top,
            arg0 = arg0,
            resume = if resume_target { 1 } else { 0 },
        ),
    );
    sel4_guard::uart_breadcrumb(guard_stage, "seL4_TCB_WriteRegisters", breadcrumb.as_str());
    let regs = sel4_sys::seL4_UserContext {
        pc: entry as seL4_Word,
        sp: stack_top as seL4_Word,
        spsr: 0,
        x0: arg0,
        x1: 0,
        x2: 0,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
        x8: 0,
        x16: 0,
        x17: 0,
        x18: 0,
        x29: 0,
        x30: 0,
        x9: 0,
        x10: 0,
        x11: 0,
        x12: 0,
        x13: 0,
        x14: 0,
        x15: 0,
        x19: 0,
        x20: 0,
        x21: 0,
        x22: 0,
        x23: 0,
        x24: 0,
        x25: 0,
        x26: 0,
        x27: 0,
        x28: 0,
        tpidr_el0: 0,
        tpidrro_el0: 0,
    };
    let resume: sel4_sys::seL4_Bool = if resume_target { 1 } else { 0 };
    // SAFETY: The register block is fully initialized and lives for the duration of the
    // syscall. seL4 validates the TCB capability and register count.
    let result = unsafe {
        sel4_sys::seL4_TCB_WriteRegisters(
            guarded_tcb,
            resume,
            0,
            sel4_sys::SEL4_AARCH64_USER_CONTEXT_REGISTER_COUNT,
            &regs as *const _,
        )
    };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] write-registers failed tcb=0x{tcb:04x} entry=0x{entry:016x} sp=0x{sp:016x} err={err} ({name})",
            tcb = guarded_tcb,
            entry = entry,
            sp = stack_top,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Suspends a TCB.
#[cfg(feature = "kernel")]
pub fn suspend_tcb(tcb_cap: seL4_CPtr) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.Suspend";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let result = suspend_tcb_syscall(guarded_tcb);
    if let Err(error) = result {
        ::log::error!(
            "[tcb] suspend failed tcb=0x{tcb:04x} err={error} ({name})",
            tcb = guarded_tcb,
            name = error_name(error),
        );
    }
    result
}

/// Suspend one TCB without diagnostic formatting or logger acquisition.
#[cfg(feature = "kernel")]
pub(crate) fn suspend_tcb_bounded(tcb_cap: seL4_CPtr) -> Result<(), seL4_Error> {
    suspend_tcb_syscall(tcb_cap)
}

#[cfg(feature = "kernel")]
#[inline(always)]
fn suspend_tcb_syscall(guarded_tcb: seL4_CPtr) -> Result<(), seL4_Error> {
    // SAFETY: The guarded CPtr is a TCB capability; seL4 validates the operation.
    let result = unsafe { sel4_sys::seL4_TCB_Suspend(guarded_tcb) };
    if result == seL4_NoError {
        Ok(())
    } else {
        Err(result)
    }
}

/// Resumes a suspended TCB.
#[cfg(feature = "kernel")]
pub fn resume_tcb(tcb_cap: seL4_CPtr) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.Resume";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let mut breadcrumb = HeaplessString::<64>::new();
    let _ = fmt::write(
        &mut breadcrumb,
        format_args!("tcb=0x{tcb:04x}", tcb = guarded_tcb),
    );
    sel4_guard::uart_breadcrumb(guard_stage, "seL4_TCB_Resume", breadcrumb.as_str());
    // SAFETY: The guarded CPtr is a TCB capability; seL4 validates the operation.
    let result = unsafe { sel4_sys::seL4_TCB_Resume(guarded_tcb) };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] resume failed tcb=0x{tcb:04x} err={err} ({name})",
            tcb = guarded_tcb,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Binds a notification object to a TCB.
#[cfg(feature = "kernel")]
pub fn bind_tcb_notification(
    tcb_cap: seL4_CPtr,
    notification_cap: seL4_CPtr,
) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.BindNotification";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    let guarded_notification =
        sel4_guard::guard_cptr(guard_stage, "notification_cap", notification_cap);
    let mut breadcrumb = HeaplessString::<96>::new();
    let _ = fmt::write(
        &mut breadcrumb,
        format_args!(
            "tcb=0x{tcb:04x} notification=0x{notification:04x}",
            tcb = guarded_tcb,
            notification = guarded_notification,
        ),
    );
    sel4_guard::uart_breadcrumb(
        guard_stage,
        "seL4_TCB_BindNotification",
        breadcrumb.as_str(),
    );
    // SAFETY: The guarded CPtrs are kernel capabilities supplied by bootstrap code; seL4
    // validates object types and binding state.
    let result = unsafe { sel4_sys::seL4_TCB_BindNotification(guarded_tcb, guarded_notification) };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] bind-notification failed tcb=0x{tcb:04x} notification=0x{notification:04x} err={err} ({name})",
            tcb = guarded_tcb,
            notification = guarded_notification,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Unbinds any notification object from a TCB.
#[cfg(feature = "kernel")]
pub fn unbind_tcb_notification(tcb_cap: seL4_CPtr) -> Result<(), seL4_Error> {
    let guard_stage = "TCB.UnbindNotification";
    let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
    // SAFETY: The guarded CPtr is a TCB capability; seL4 validates binding state.
    let result = unsafe { sel4_sys::seL4_TCB_UnbindNotification(guarded_tcb) };
    if result == seL4_NoError {
        Ok(())
    } else {
        ::log::error!(
            "[tcb] unbind-notification failed tcb=0x{tcb:04x} err={err} ({name})",
            tcb = guarded_tcb,
            err = result,
            name = error_name(result),
        );
        Err(result)
    }
}

/// Safe projection of `seL4_CNode_Copy` for bootstrap modules.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_copy(
    _bootinfo: &seL4_BootInfo,
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    rights: sel4_sys::seL4_CapRights,
) -> seL4_Error {
    debug_put_char(b'C' as i32);
    let depth_bits = _bootinfo.init_cnode_depth();
    let depth_word: seL4_Word = depth_bits.try_into().expect("init cnode depth fits in u8");
    unsafe {
        seL4_CNode_Copy(
            dest_root,
            dest_index,
            depth_word,
            src_root,
            src_index,
            depth_word,
            sel4_sys::seL4_CapRights_to_word(rights),
        )
    }
}

/// Safe projection of `seL4_CNode_Copy` when both invocations target precomputed depths.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_copy_depth(
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    dest_depth: u8,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    src_depth: u8,
    rights: sel4_sys::seL4_CapRights,
) -> seL4_Error {
    #[cfg(target_os = "none")]
    {
        let dest_depth_word: seL4_Word = dest_depth.into();
        let src_depth_word: seL4_Word = src_depth.into();
        // SAFETY: Callers must ensure that the provided CNodes and depths originate from
        // kernel-supplied boot information. This wrapper centralises the unsafe invocation so
        // higher-level modules can remain within the crate-wide `#![deny(unsafe_code)]` policy.
        unsafe {
            seL4_CNode_Copy(
                dest_root,
                dest_index,
                dest_depth_word,
                src_root,
                src_index,
                src_depth_word,
                sel4_sys::seL4_CapRights_to_word(rights),
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (
            dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights,
        );
        seL4_NoError
    }
}

/// Safe projection of `seL4_CNode_Move` between explicitly bounded CNodes.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_move_depth(
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    dest_depth: u8,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    src_depth: u8,
) -> seL4_Error {
    #[cfg(target_os = "none")]
    {
        let dest_depth_word: seL4_Word = dest_depth.into();
        let src_depth_word: seL4_Word = src_depth.into();
        // SAFETY: Callers supply validated CNode capabilities, empty
        // destination slots, and explicit depths. seL4 preserves the moved
        // capability's MDB ancestry, which the driver-supervisor generation
        // anchor relies on for later bounded revoke.
        unsafe {
            seL4_CNode_Move(
                dest_root,
                dest_index,
                dest_depth_word,
                src_root,
                src_index,
                src_depth_word,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (
            dest_root, dest_index, dest_depth, src_root, src_index, src_depth,
        );
        seL4_NoError
    }
}

/// Safe projection of `seL4_CNode_Delete` for bootstrap modules.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_delete(root: seL4_CNode, index: seL4_CPtr, depth: u8) -> seL4_Error {
    debug_put_char(b'C' as i32);
    cnode_delete_bounded(root, index, depth)
}

/// Perform one validated CNode delete without a debug or logging side effect.
#[cfg(feature = "kernel")]
#[inline(always)]
pub(crate) fn cnode_delete_bounded(root: seL4_CNode, index: seL4_CPtr, depth: u8) -> seL4_Error {
    let depth_word: seL4_Word = depth.into();
    // SAFETY: Callers provide a valid CNode root/index/depth triple from bootstrap-owned caps;
    // seL4 validates the addressed slot.
    unsafe { seL4_CNode_Delete(root, index, depth_word) }
}

/// Safe projection of `seL4_CNode_Revoke` for driver-task cap rollback.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_revoke(root: seL4_CNode, index: seL4_CPtr, depth: u8) -> seL4_Error {
    let depth_word: seL4_Word = depth.into();
    // SAFETY: Callers provide a valid CNode root/index/depth triple from bootstrap-owned caps;
    // seL4 validates the addressed slot before revoking descendants.
    unsafe { seL4_CNode_Revoke(root, index, depth_word) }
}

/// Creates a level-triggered IRQ handler capability in the supplied init-root slot.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn irq_control_get_level_handler(
    irq: seL4_Word,
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    dest_depth: u8,
) -> seL4_Error {
    #[cfg(all(target_arch = "aarch64", target_os = "none"))]
    {
        let trigger_result =
            irq_control_get_trigger_handler(irq, 0, dest_root, dest_index, dest_depth);
        if trigger_result != sel4_sys::seL4_IllegalOperation {
            return trigger_result;
        }
    }

    #[cfg(target_os = "none")]
    {
        let mut mr0 = irq;
        let mut mr1 = dest_index;
        let mut mr2 = seL4_Word::from(dest_depth);
        let mut mr3 = 0;

        // SAFETY: The destination CNode/depth come from kernel bootinfo-derived
        // slots, and the call shape mirrors libsel4's IRQControl_Get wrapper.
        unsafe {
            sel4_sys::seL4_SetCap(0, dest_root);
            let tag = sel4_sys::seL4_MessageInfo::new(
                sel4_sys::invocation_label_IRQIssueIRQHandler as seL4_Word,
                0,
                1,
                3,
            );
            let output = sel4_sys::seL4_CallWithMRs(
                sel4_sys::seL4_CapIRQControl,
                tag,
                &mut mr0,
                &mut mr1,
                &mut mr2,
                &mut mr3,
            );
            let result = sel4_sys::seL4_MessageInfo_get_label(output) as seL4_Error;
            if result != seL4_NoError {
                sel4_sys::seL4_SetMR(0, mr0);
                sel4_sys::seL4_SetMR(1, mr1);
                sel4_sys::seL4_SetMR(2, mr2);
                sel4_sys::seL4_SetMR(3, mr3);
            }
            result
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (irq, dest_root, dest_index, dest_depth);
        seL4_NoError
    }
}

/// Creates an IRQ handler capability with an explicit ARM trigger type.
///
/// The seL4 ARM API uses `trigger = 1` for edge-triggered and `trigger = 0` for
/// level-triggered IRQs. Pi 4 PCIe INTx-style lines must be level-triggered.
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
#[inline(always)]
pub fn irq_control_get_trigger_handler(
    irq: seL4_Word,
    trigger: seL4_Word,
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    dest_depth: u8,
) -> seL4_Error {
    let mut mr0 = irq;
    let mut mr1 = trigger;
    let mut mr2 = dest_index;
    let mut mr3 = seL4_Word::from(dest_depth);

    // SAFETY: The message register layout and capability slot match libsel4's
    // generated seL4_IRQControl_GetTrigger wrapper for ARM.
    unsafe {
        sel4_sys::seL4_SetCap(0, dest_root);
        let tag = sel4_sys::seL4_MessageInfo::new(
            sel4_sys::arch_invocation_label_ARMIRQIssueIRQHandlerTrigger as seL4_Word,
            0,
            1,
            4,
        );
        let output = sel4_sys::seL4_CallWithMRs(
            sel4_sys::seL4_CapIRQControl,
            tag,
            &mut mr0,
            &mut mr1,
            &mut mr2,
            &mut mr3,
        );
        let result = sel4_sys::seL4_MessageInfo_get_label(output) as seL4_Error;
        if result != seL4_NoError {
            sel4_sys::seL4_SetMR(0, mr0);
            sel4_sys::seL4_SetMR(1, mr1);
            sel4_sys::seL4_SetMR(2, mr2);
            sel4_sys::seL4_SetMR(3, mr3);
        }
        result
    }
}

/// Binds an IRQ handler capability to a notification object.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn irq_handler_set_notification(handler: seL4_CPtr, notification: seL4_CPtr) -> seL4_Error {
    #[cfg(target_os = "none")]
    unsafe {
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;
        sel4_sys::seL4_SetCap(0, notification);
        let tag = sel4_sys::seL4_MessageInfo::new(
            sel4_sys::invocation_label_IRQSetIRQHandler as seL4_Word,
            0,
            1,
            0,
        );
        let output =
            sel4_sys::seL4_CallWithMRs(handler, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = sel4_sys::seL4_MessageInfo_get_label(output) as seL4_Error;
        if result != seL4_NoError {
            sel4_sys::seL4_SetMR(0, mr0);
            sel4_sys::seL4_SetMR(1, mr1);
            sel4_sys::seL4_SetMR(2, mr2);
            sel4_sys::seL4_SetMR(3, mr3);
        }
        result
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (handler, notification);
        seL4_NoError
    }
}

/// Acknowledges a serviced interrupt and re-enables delivery for its handler.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn irq_handler_ack(handler: seL4_CPtr) -> seL4_Error {
    #[cfg(target_os = "none")]
    {
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;
        // SAFETY: The call shape mirrors libsel4's IRQHandler_Ack wrapper and
        // uses the caller-provided IRQHandler cap as the invocation target.
        unsafe {
            let tag = sel4_sys::seL4_MessageInfo::new(
                sel4_sys::invocation_label_IRQAckIRQ as seL4_Word,
                0,
                0,
                0,
            );
            let output =
                sel4_sys::seL4_CallWithMRs(handler, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
            let result = sel4_sys::seL4_MessageInfo_get_label(output) as seL4_Error;
            if result != seL4_NoError {
                sel4_sys::seL4_SetMR(0, mr0);
                sel4_sys::seL4_SetMR(1, mr1);
                sel4_sys::seL4_SetMR(2, mr2);
                sel4_sys::seL4_SetMR(3, mr3);
            }
            result
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = handler;
        seL4_NoError
    }
}

/// Clears the notification binding from an IRQ handler capability.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn irq_handler_clear(handler: seL4_CPtr) -> seL4_Error {
    #[cfg(target_os = "none")]
    unsafe {
        let mut mr0 = 0;
        let mut mr1 = 0;
        let mut mr2 = 0;
        let mut mr3 = 0;
        let tag = sel4_sys::seL4_MessageInfo::new(
            sel4_sys::invocation_label_IRQClearIRQHandler as seL4_Word,
            0,
            0,
            0,
        );
        let output =
            sel4_sys::seL4_CallWithMRs(handler, tag, &mut mr0, &mut mr1, &mut mr2, &mut mr3);
        let result = sel4_sys::seL4_MessageInfo_get_label(output) as seL4_Error;
        if result != seL4_NoError {
            sel4_sys::seL4_SetMR(0, mr0);
            sel4_sys::seL4_SetMR(1, mr1);
            sel4_sys::seL4_SetMR(2, mr2);
            sel4_sys::seL4_SetMR(3, mr3);
        }
        result
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = handler;
        seL4_NoError
    }
}

/// Safe projection of `seL4_CNode_Mint` for bootstrap modules.
#[cfg(feature = "kernel")]
#[deprecated(note = "use cspace_sys::*_invoc")]
#[inline(always)]
pub(crate) fn cnode_mint(
    _bootinfo: &seL4_BootInfo,
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    rights: sel4_sys::seL4_CapRights,
    badge: seL4_Word,
) -> seL4_Error {
    debug_put_char(b'C' as i32);
    let depth_bits = _bootinfo.init_cnode_depth();
    let depth_word: seL4_Word = depth_bits.try_into().expect("init cnode depth fits in u8");
    unsafe {
        seL4_CNode_Mint(
            dest_root, dest_index, depth_word, src_root, src_index, depth_word, rights, badge,
        )
    }
}

/// Safe projection of `seL4_CNode_Mint` when both invocations target precomputed depths.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_mint_depth(
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    dest_depth: u8,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    src_depth: u8,
    rights: sel4_sys::seL4_CapRights,
    badge: seL4_Word,
) -> seL4_Error {
    #[cfg(target_os = "none")]
    {
        let dest_depth_word: seL4_Word = dest_depth.into();
        let src_depth_word: seL4_Word = src_depth.into();
        // SAFETY: Callers guarantee that the provided indices and depths stem from the
        // kernel-advertised CSpace topology, ensuring the kernel accepts the invocation.
        unsafe {
            seL4_CNode_Mint(
                dest_root,
                dest_index,
                dest_depth_word,
                src_root,
                src_index,
                src_depth_word,
                rights,
                badge,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (
            dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights, badge,
        );
        seL4_NoError
    }
}

/// Issues a checked `seL4_CNode_Mint`, logging any non-zero return code.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_mint_checked(
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    dest_depth: u8,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    src_depth: u8,
    rights: sel4_sys::seL4_CapRights,
    badge: seL4_Word,
) -> Result<(), i32> {
    #[cfg(target_os = "none")]
    {
        let rc = unsafe {
            seL4_CNode_Mint(
                dest_root,
                dest_index,
                dest_depth as seL4_Word,
                src_root,
                src_index,
                src_depth as seL4_Word,
                rights,
                badge,
            )
        };
        crate::bootstrap::ktry("cnode.mint", rc as i32)
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (
            dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights, badge,
        );
        Ok(())
    }
}

/// Attempts to retrieve a byte from the seL4 debug console without blocking.
///
/// Returns the pending byte when input is available or `-1` when the console
/// has no buffered input. The function behaves identically across the
/// platform-specific implementations compiled below.
#[cfg(all(feature = "kernel", feature = "debug-input", target_arch = "aarch64"))]
#[inline(always)]
pub fn debug_poll_char() -> i32 {
    if serial::serial_root_uart_released_for_linked_runtime() {
        return -1;
    }
    // SAFETY: `seL4_DebugPollChar` is provided by the seL4 kernel on targets that expose
    // the debug console polling syscall. The call has no side effects besides returning the
    // pending byte or a negative sentinel when no input is available.
    unsafe { sel4_debug_poll_char() }
}

/// Attempts to retrieve a byte from the seL4 debug console without blocking.
///
/// Returns `-1` to signal that the console does not support polling on the
/// current architecture.
#[cfg(all(
    feature = "kernel",
    feature = "debug-input",
    not(target_arch = "aarch64")
))]
#[inline(always)]
pub fn debug_poll_char() -> i32 {
    // Some seL4 architectures do not surface a debug polling syscall. Retain the existing
    // behaviour and report that no input is pending.
    -1
}

/// Attempts to retrieve a byte from the seL4 debug console without blocking.
///
/// Returns `-1` because the build configuration does not enable the
/// `debug-input` feature or because the code is executing in host mode.
#[cfg(not(all(feature = "kernel", feature = "debug-input")))]
#[inline(always)]
pub fn debug_poll_char() -> i32 {
    // Without the `debug-input` feature (or when compiling in host mode) the debug console
    // remains write-only. Preserve the historical behaviour by signalling no pending input.
    -1
}

#[cfg(all(feature = "kernel", feature = "debug-input", target_arch = "aarch64"))]
#[inline(always)]
unsafe fn sel4_debug_poll_char() -> i32 {
    extern "C" {
        fn seL4_DebugPollChar() -> i32;
    }

    unsafe { seL4_DebugPollChar() }
}

fn objtype_name(t: seL4_Word) -> &'static str {
    match t {
        x if x == SEL4_UNTYPED_OBJECT_WORD => "seL4_UntypedObject",
        x if x == SEL4_TCB_OBJECT_WORD => "seL4_TCBObject",
        x if x == SEL4_ENDPOINT_OBJECT_WORD => "seL4_EndpointObject",
        x if x == SEL4_NOTIFICATION_OBJECT_WORD => "seL4_NotificationObject",
        x if x == SEL4_CAP_TABLE_OBJECT_WORD => "seL4_CapTableObject",
        x if x == SEL4_ARM_PAGE_OBJECT_WORD => "seL4_ARM_Page",
        x if x == SEL4_ARM_LARGE_PAGE_OBJECT_WORD => "seL4_ARM_LargePage",
        x if x == SEL4_ARM_PAGE_TABLE_OBJECT_WORD => "seL4_ARM_PageTableObject",
        x if x == SEL4_ARM_VSPACE_OBJECT_WORD => "seL4_ARM_VSpaceObject",
        _ => "<?>",
    }
}

/// Converts an [`seL4_Error`] into its symbolic name for human-readable diagnostics.
#[must_use]
pub fn error_name(err: seL4_Error) -> &'static str {
    match err {
        sel4_sys::seL4_NoError => "seL4_NoError",
        sel4_sys::seL4_InvalidArgument => "seL4_InvalidArgument",
        sel4_sys::seL4_InvalidCapability => "seL4_InvalidCapability",
        sel4_sys::seL4_IllegalOperation => "seL4_IllegalOperation",
        sel4_sys::seL4_RangeError => "seL4_RangeError",
        sel4_sys::seL4_AlignmentError => "seL4_AlignmentError",
        sel4_sys::seL4_FailedLookup => "seL4_FailedLookup",
        sel4_sys::seL4_TruncatedMessage => "seL4_TruncatedMessage",
        sel4_sys::seL4_DeleteFirst => "seL4_DeleteFirst",
        sel4_sys::seL4_RevokeFirst => "seL4_RevokeFirst",
        sel4_sys::seL4_NotEnoughMemory => "seL4_NotEnoughMemory",
        _ => "seL4_UnknownError",
    }
}

/// Converts a [`seL4_ObjectType`] into its symbolic name for diagnostics.
#[must_use]
pub fn object_type_name(object_type: seL4_ObjectType) -> &'static str {
    match object_type {
        sel4_sys::seL4_UntypedObjectType => "seL4_UntypedObject",
        sel4_sys::seL4_TCBObjectType => "seL4_TCBObject",
        sel4_sys::seL4_EndpointObjectType => "seL4_EndpointObject",
        sel4_sys::seL4_NotificationObjectType => "seL4_NotificationObject",
        sel4_sys::seL4_CapTableObjectType => "seL4_CapTableObject",
        sel4_sys::seL4_ARM_PageObjectType => "seL4_ARM_Page",
        sel4_sys::seL4_ARM_LargePageObjectType => "seL4_ARM_LargePage",
        sel4_sys::seL4_ARM_PageTableObjectType => "seL4_ARM_PageTableObject",
        sel4_sys::seL4_ARM_VSpaceObjectType => "seL4_ARM_VSpaceObject",
        _ => "<?>",
    }
}

#[cfg(all(feature = "kernel", not(target_arch = "aarch64")))]
compile_error!("This path currently expects AArch64; wire correct ARM object types for your arch.");

const _: () = {
    let _check: [u8; core::mem::size_of::<seL4_Word>()] = [0; core::mem::size_of::<usize>()];
};

/// Extension trait exposing bootinfo fields and derived values used by the root task.
pub trait BootInfoExt {
    /// Returns the writable init thread CNode capability exposed via the initial CSpace root slot.
    fn init_cnode_cap(&self) -> seL4_CPtr;
    /// Returns the canonical (guard-less) init CNode capability provided by the kernel.
    fn canonical_root_cap(&self) -> seL4_CPtr;

    /// Returns the initial thread's TCB capability slot.
    fn init_tcb_cap(&self) -> seL4_CPtr;

    /// Returns the radix depth (in bits) of the init thread's root CNode.
    fn init_cnode_depth(&self) -> u8;

    /// Returns the number of bits describing the capacity of the init thread's CSpace root.
    fn init_cnode_bits(&self) -> usize;

    /// Returns the first slot index within the bootinfo-declared empty slot window.
    fn empty_first_slot(&self) -> usize;

    /// Returns the exclusive upper bound of the bootinfo-declared empty slot window.
    fn empty_last_slot_excl(&self) -> usize;

    /// Returns the bootinfo-advertised empty slot window as `usize` values.
    fn init_cnode_empty_usize(&self) -> (usize, usize);
    /// Returns the slot range containing extra bootinfo pages.
    fn extra_bipage_slots(&self) -> (seL4_CPtr, seL4_CPtr);

    /// Returns the raw bytes that make up the bootinfo header.
    fn header_bytes(&self) -> &[u8];

    /// Returns the extra bootinfo region emitted by the kernel as a byte slice.
    fn extra_bytes(&self) -> &[u8];

    /// Returns the init thread's IPC buffer pointer when supplied by the kernel.
    fn ipc_buffer_ptr(&self) -> Option<NonNull<sel4_sys::seL4_IPCBuffer>>;
}

impl BootInfoExt for seL4_BootInfo {
    #[inline(always)]
    fn init_cnode_cap(&self) -> seL4_CPtr {
        seL4_CapInitThreadCNode
    }

    #[inline(always)]
    fn canonical_root_cap(&self) -> seL4_CPtr {
        canonical_root_cap_ptr()
    }

    #[inline(always)]
    fn init_tcb_cap(&self) -> seL4_CPtr {
        seL4_CapInitThreadTCB
    }

    #[inline(always)]
    fn init_cnode_depth(&self) -> u8 {
        init_cnode_depth(self)
    }

    #[inline(always)]
    fn init_cnode_bits(&self) -> usize {
        canonical_cnode_bits(self) as usize
    }

    #[inline(always)]
    fn empty_first_slot(&self) -> usize {
        self.empty.start as usize
    }

    #[inline(always)]
    fn empty_last_slot_excl(&self) -> usize {
        self.empty.end as usize
    }

    #[inline(always)]
    fn init_cnode_empty_usize(&self) -> (usize, usize) {
        (self.empty_first_slot(), self.empty_last_slot_excl())
    }

    #[inline(always)]
    fn extra_bipage_slots(&self) -> (seL4_CPtr, seL4_CPtr) {
        (
            self.extraBIPages.start as seL4_CPtr,
            self.extraBIPages.end as seL4_CPtr,
        )
    }

    #[inline(always)]
    fn header_bytes(&self) -> &[u8] {
        let header = core::slice::from_ref(self);
        let (prefix, bytes, suffix) = unsafe {
            // SAFETY: `u8` has an alignment requirement of 1, therefore every
            // possible pointer value is aligned for `u8`. The slice produced by
            // `from_ref` is naturally aligned for `seL4_BootInfo`, so casting it
            // to `u8` elements cannot violate alignment guarantees.
            header.align_to::<u8>()
        };
        debug_assert!(prefix.is_empty(), "bootinfo header must be aligned to u8");
        debug_assert!(
            suffix.is_empty(),
            "bootinfo header must not leave trailing padding"
        );
        bytes
    }

    fn extra_bytes(&self) -> &[u8] {
        match bootinfo_extra_slice(self, BOOTINFO_FRAME_BYTES) {
            Ok((slice, _, _, _)) => slice,
            Err(err) => {
                log::error!("invalid bootinfo extra region: {err}");
                &[]
            }
        }
    }

    fn ipc_buffer_ptr(&self) -> Option<NonNull<sel4_sys::seL4_IPCBuffer>> {
        NonNull::new(self.ipcBuffer)
    }
}

/// Emits a concise dump of raw bootinfo parameters to aid debugging early boot wiring mistakes.
pub fn bootinfo_debug_dump(view: &BootInfoView) {
    let header = view.header();
    let init_bits = header.init_cnode_bits();
    log::info!(
        "[cohesix:root-task] bootinfo.raw: initCNode=0x{:x} initBits={} empty=[0x{:04x}..0x{:04x})",
        view.root_cnode_cap(),
        init_bits,
        header.empty_first_slot(),
        header.empty_last_slot_excl()
    );
    debug_assert!(init_bits > 0, "BootInfo initBits is 0 — capacity invalid");
}

#[inline(always)]
pub fn debug_dump_scheduler() {
    #[cfg(target_os = "none")]
    // SAFETY: This is the seL4 debug scheduler syscall and takes no user
    // pointers or capability arguments.
    unsafe {
        sel4_sys::seL4_DebugDumpScheduler();
    }
    #[cfg(not(target_os = "none"))]
    sel4_sys::seL4_DebugDumpScheduler();
}

#[inline(always)]
pub fn debug_dump_cpu_info() {
    #[cfg(target_os = "none")]
    // SAFETY: This is the seL4 debug CPU-info syscall and takes no user
    // pointers or capability arguments.
    unsafe {
        sel4_sys::seL4_DebugDumpCPUInfo();
    }
    #[cfg(not(target_os = "none"))]
    sel4_sys::seL4_DebugDumpCPUInfo();
}

const BOOTINFO_WINDOW_CANARY_PRE: u64 = 0xd00d_f00d_5a5a_cafe;
const BOOTINFO_WINDOW_CANARY_POST: u64 = 0xface_feed_beef_c0de;
const BOOTINFO_WINDOW_GUARD_ENABLED: bool = cfg!(any(
    debug_assertions,
    feature = "net-console",
    feature = "net-diag"
));
static mut BOOTINFO_WINDOW_STORAGE: BootinfoWindow = BootinfoWindow { start: 0, end: 0 };

#[derive(Clone, Copy)]
struct BootinfoWindowState {
    window_ptr: *const BootinfoWindow,
    window_addr: usize,
    expected: BootinfoWindow,
    capacity: usize,
    bootinfo_ptr: usize,
    bootinfo_empty_ptr: usize,
    snapshot_ptr: Option<usize>,
    snapshot_window_ptr: Option<usize>,
    snapshot_empty_ptr: Option<usize>,
    pre_canary: u64,
    post_canary: u64,
}

impl BootinfoWindowState {
    fn new(
        window_ptr: *const BootinfoWindow,
        expected: BootinfoWindow,
        capacity: usize,
        bootinfo_ptr: usize,
        bootinfo_empty_ptr: usize,
        snapshot_ptr: Option<usize>,
        snapshot_window_ptr: Option<usize>,
        snapshot_empty_ptr: Option<usize>,
    ) -> Self {
        Self {
            window_ptr,
            window_addr: window_ptr as usize,
            expected,
            capacity,
            bootinfo_ptr,
            bootinfo_empty_ptr,
            snapshot_ptr,
            snapshot_window_ptr,
            snapshot_empty_ptr,
            pre_canary: BOOTINFO_WINDOW_CANARY_PRE,
            post_canary: BOOTINFO_WINDOW_CANARY_POST,
        }
    }
}

// SAFETY: The bootinfo window state references a static BootinfoWindow storage region that
// lives for the duration of the root task. The guard only reads this window snapshot, so
// sharing the pointer across threads is safe.
unsafe impl Send for BootinfoWindowState {}

pub struct BootinfoWindowGuard {
    state: SpinMutex<Option<BootinfoWindowState>>,
    armed: AtomicBool,
    reported: AtomicBool,
}

impl BootinfoWindowGuard {
    pub const fn new() -> Self {
        Self {
            state: SpinMutex::new(None),
            armed: AtomicBool::new(false),
            reported: AtomicBool::new(false),
        }
    }

    fn enabled(&self) -> bool {
        BOOTINFO_WINDOW_GUARD_ENABLED
    }

    pub fn arm(&self, bootinfo: &seL4_BootInfo) {
        if !self.enabled() {
            return;
        }
        let capacity = 1usize
            .checked_shl(bootinfo.init_cnode_bits() as u32)
            .unwrap_or(usize::MAX);
        let expected = BootinfoWindow {
            start: bootinfo.empty.start,
            end: bootinfo.empty.end,
        };
        let bootinfo_ptr = bootinfo as *const _ as usize;
        let bootinfo_empty_ptr = core::ptr::addr_of!(bootinfo.empty) as usize;
        unsafe {
            BOOTINFO_WINDOW_STORAGE = expected;
        }
        let window_ptr: *const BootinfoWindow =
            core::ptr::addr_of!(bootinfo.empty) as *const BootinfoWindow;
        let snapshot_state = BootInfoState::get();
        let snapshot_ptr = snapshot_state.map(|state| state.snapshot_ptr() as usize);
        let snapshot_window_ptr = snapshot_state.map(|state| state.snapshot_window_ptr() as usize);
        let snapshot_empty_ptr = snapshot_state.map(|state| state.snapshot_empty_ptr() as usize);
        let mut slot = self.state.lock();
        *slot = Some(BootinfoWindowState::new(
            window_ptr,
            expected,
            capacity,
            bootinfo_ptr,
            bootinfo_empty_ptr,
            snapshot_ptr,
            snapshot_window_ptr,
            snapshot_empty_ptr,
        ));
        if let Some(state) = slot.as_ref() {
            self.log_pointer_candidates(state, "arm");
        }
        watch_range(
            "bootinfo.window",
            bootinfo_empty_ptr as *const u8,
            mem::size_of::<BootinfoWindow>(),
        );
        self.armed.store(true, Ordering::Release);
    }

    pub fn watched_region(&self) -> Option<(*const u8, usize)> {
        let slot = self.state.lock();
        slot.as_ref()
            .map(|state| (state.window_ptr.cast(), mem::size_of::<BootinfoWindow>()))
    }

    fn hexdump_window(&self, ptr: *const BootinfoWindow) -> HeaplessString<192> {
        let mut line = HeaplessString::<192>::new();
        let dump_len = mem::size_of::<BootinfoWindow>().min(32);
        let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, dump_len) };
        for byte in bytes {
            if write!(&mut line, "{byte:02x}").is_err() {
                break;
            }
        }
        line
    }

    fn log_pointer_candidates(&self, state: &BootinfoWindowState, marker: &'static str) {
        let observed = if state.window_ptr.is_null() {
            None
        } else {
            Some(unsafe { &*state.window_ptr })
        };
        let mut line = HeaplessString::<320>::new();
        let _ = write!(
            &mut line,
            "[bootinfo.window.pointers] marker={marker} bootinfo=0x{bootinfo:016x} bootinfo.empty=0x{empty:016x} guard.window=0x{window:016x} expected=[0x{start:04x}..0x{end:04x}) capacity=0x{cap:04x}",
            bootinfo = state.bootinfo_ptr,
            empty = state.bootinfo_empty_ptr,
            window = state.window_ptr as usize,
            start = state.expected.start,
            end = state.expected.end,
            cap = state.capacity
        );
        if let Some(observed_window) = observed {
            let _ = write!(
                &mut line,
                " observed=[0x{obs_start:04x}..0x{obs_end:04x})",
                obs_start = observed_window.start,
                obs_end = observed_window.end
            );
        }
        if let Some(snapshot_ptr) = state.snapshot_ptr {
            let _ = write!(
                &mut line,
                " snapshot=0x{snapshot:016x}",
                snapshot = snapshot_ptr
            );
        }
        if let Some(snapshot_window_ptr) = state.snapshot_window_ptr {
            let _ = write!(
                &mut line,
                " snapshot.window=0x{ptr:016x}",
                ptr = snapshot_window_ptr
            );
        }
        if let Some(snapshot_empty_ptr) = state.snapshot_empty_ptr {
            let _ = write!(
                &mut line,
                " snapshot.empty=0x{ptr:016x}",
                ptr = snapshot_empty_ptr
            );
        }
        boot_log::force_uart_line(line.as_str());
        log::info!("{}", line.as_str());
    }

    fn log_forensics(
        &self,
        state: &BootinfoWindowState,
        start: seL4_CPtr,
        end: seL4_CPtr,
        marker: &'static str,
    ) {
        self.log_pointer_candidates(state, marker);
        if self
            .reported
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let hexdump = self.hexdump_window(state.window_ptr);
        let mut line = HeaplessString::<256>::new();
        let _ = write!(
            &mut line,
            "[bootinfo.window.guard] marker={marker} ptr=0x{ptr:016x} start=0x{start:04x} end=0x{end:04x} expected_start=0x{exp_start:04x} expected_end=0x{exp_end:04x} capacity=0x{cap:04x} canary_pre=0x{pre:016x} canary_post=0x{post:016x} bytes=",
            ptr = state.window_ptr as usize,
            exp_start = state.expected.start,
            exp_end = state.expected.end,
            cap = state.capacity,
            pre = state.pre_canary,
            post = state.post_canary,
        );
        let _ = write!(&mut line, "{hexdump}");
        if let Some(hint) =
            watch_hint_for(state.window_ptr as usize, mem::size_of::<BootinfoWindow>())
        {
            if let Some(context) = hint.context {
                let _ = write!(&mut line, " nearest_writer={context}");
                if let (Some(file), Some(line_no)) = (hint.location_file, hint.location_line) {
                    let _ = write!(&mut line, " at {file}:{line_no}");
                }
            } else {
                let _ = write!(&mut line, " nearest_writer_label={}", hint.label);
            }
        }
        boot_log::force_uart_line(line.as_str());
        log::error!("{}", line.as_str());
    }

    pub fn check(&self, marker: &'static str) {
        if !self.enabled() || !self.armed.load(Ordering::Acquire) {
            return;
        }
        let state = { self.state.lock().clone() };
        let Some(state) = state else {
            return;
        };
        if state.window_ptr.is_null() || state.window_addr == 0 {
            self.log_pointer_candidates(&state, marker);
            panic!("bootinfo window pointer invalid (detected at {marker})");
        }
        if state.window_ptr as usize != state.window_addr {
            self.log_pointer_candidates(&state, marker);
            panic!("bootinfo window pointer invalid (detected at {marker})");
        }
        let window = unsafe { &*state.window_ptr };
        let start = window.start;
        let end = window.end;
        let start_end_valid =
            start <= end && start >= state.expected.start && (end as usize) <= state.capacity;
        let start_end_expected = start == state.expected.start && end == state.expected.end;
        let canaries_ok = state.pre_canary == BOOTINFO_WINDOW_CANARY_PRE
            && state.post_canary == BOOTINFO_WINDOW_CANARY_POST;
        if start_end_expected && start_end_valid && canaries_ok {
            return;
        }
        if !start_end_valid {
            self.log_forensics(&state, start, end, marker);
            panic!("bootinfo window range invalid (detected at {marker})");
        }

        self.log_forensics(&state, start, end, marker);
        panic!("bootinfo window corrupted (detected at {marker})");
    }
}

pub static BOOTINFO_WINDOW_GUARD: BootinfoWindowGuard = BootinfoWindowGuard::new();

#[inline(always)]
fn ranges_overlap_usize(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

#[inline(always)]
fn bootinfo_watch_range() -> Option<Range<usize>> {
    BOOTINFO_WINDOW_GUARD
        .watched_region()
        .map(|(ptr, len)| ptr as usize..ptr as usize + len)
}

#[track_caller]
pub fn store_u64_watched(dst: *mut u64, val: u64, ctx: &'static str) {
    if let Some(range) = bootinfo_watch_range() {
        let dst_range = dst as usize..dst as usize + core::mem::size_of::<u64>();
        if ranges_overlap_usize(&dst_range, &range) {
            let allowlisted = ctx.starts_with("bootinfo.") || ctx.starts_with("test.");
            if !allowlisted {
                let ascii_bytes = val.to_ne_bytes();
                let ascii = ascii_bytes.map(|byte| {
                    if byte.is_ascii_graphic() || byte == b' ' {
                        byte
                    } else {
                        b'.'
                    }
                });
                let ascii_str = core::str::from_utf8(&ascii).unwrap_or("????????");
                let location = Location::caller();
                panic!(
                    "[bootinfo.window.store] dst=0x{dst:016x} val=0x{val:016x} ascii={ascii_str} ctx={ctx} location={file}:{line}",
                    dst = dst as usize,
                    val = val,
                    file = location.file(),
                    line = location.line(),
                );
            }
        }
    }
    unsafe { core::ptr::write(dst, val) };
}

#[track_caller]
pub fn store_bootinfo_empty_start(
    empty: &mut sel4_sys::seL4_SlotRegion,
    val: seL4_CPtr,
    ctx: &'static str,
) {
    store_u64_watched(&mut empty.start as *mut _ as *mut u64, val as u64, ctx);
}

#[track_caller]
pub fn store_bootinfo_empty_end(
    empty: &mut sel4_sys::seL4_SlotRegion,
    val: seL4_CPtr,
    ctx: &'static str,
) {
    store_u64_watched(&mut empty.end as *mut _ as *mut u64, val as u64, ctx);
}

#[track_caller]
pub fn store_bootinfo_empty_region(
    empty: &mut sel4_sys::seL4_SlotRegion,
    start: seL4_CPtr,
    end: seL4_CPtr,
    ctx: &'static str,
) {
    store_bootinfo_empty_start(empty, start, ctx);
    store_bootinfo_empty_end(empty, end, ctx);
}

pub const PAGE_BITS: usize = 12;
pub const PAGE_TABLE_BITS: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_TABLE_ALIGN: usize = 1 << 21;
const PAGE_DIRECTORY_ALIGN: usize = 1 << 30;
const PAGE_UPPER_DIRECTORY_ALIGN: usize = 1 << 39;
const DEVICE_VADDR_BASE: usize = 0xA000_0000;
const DMA_VADDR_BASE: usize = 0xB000_0000;
const DMA_LOW_GUARD_BYTES: usize = 64 * 1024 * 1024;
const MAX_PAGE_TABLES: usize = 64;
const MAX_PAGE_DIRECTORIES: usize = 32;
const MAX_PAGE_UPPER_DIRECTORIES: usize = 8;
const MAX_DRIVER_VSPACE_PAGE_TABLES: usize = 12;
const MAX_DRIVER_VSPACE_PAGE_DIRECTORIES: usize = 6;
const MAX_DRIVER_VSPACE_PAGE_UPPER_DIRECTORIES: usize = 3;
pub(crate) const DEVICE_VM_ATTRIBUTES: seL4_ARM_VMAttributes = sel4_sys::seL4_ARM_ExecuteNever;

/// Returns the exclusive virtual address range reserved for device page tables and mappings.
pub const fn device_window_range() -> core::ops::Range<usize> {
    DEVICE_VADDR_BASE..DMA_VADDR_BASE
}

#[derive(Clone, Debug)]
pub struct ReservedVaddrRanges {
    ranges: Vec<core::ops::Range<usize>, 8>,
}

impl ReservedVaddrRanges {
    pub const fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    pub fn reserve(&mut self, range: &core::ops::Range<usize>, label: &'static str) {
        self.assert_valid(range, label);
        self.assert_free(range, label);
        self.ranges
            .push(range.clone())
            .expect("reserved vaddr range capacity exceeded");
    }

    pub fn assert_free(&self, range: &core::ops::Range<usize>, label: &str) {
        if let Some(conflict) = self.first_overlap(range) {
            panic!(
                "mapping {label} range [0x{start:016x}..0x{end:016x}) overlaps reserved [0x{conflict_start:016x}..0x{conflict_end:016x})",
                start = range.start,
                end = range.end,
                conflict_start = conflict.start,
                conflict_end = conflict.end,
            );
        }
    }

    pub fn next_aligned_range(
        &self,
        start: usize,
        span: usize,
        align: usize,
    ) -> core::ops::Range<usize> {
        assert!(align.is_power_of_two(), "alignment must be a power of two");
        let mut candidate = Self::align_up(start, align);
        loop {
            let end = candidate
                .checked_add(span)
                .expect("virtual address allocation overflow");
            let range = candidate..end;
            if let Some(conflict) = self.first_overlap(&range) {
                candidate = Self::align_up(conflict.end, align);
                continue;
            }
            return range;
        }
    }

    fn first_overlap(&self, range: &core::ops::Range<usize>) -> Option<&core::ops::Range<usize>> {
        self.ranges
            .iter()
            .find(|existing| Self::ranges_overlap(existing, range))
    }

    fn ranges_overlap(a: &core::ops::Range<usize>, b: &core::ops::Range<usize>) -> bool {
        a.start < b.end && b.start < a.end
    }

    fn align_up(value: usize, align: usize) -> usize {
        (value.checked_add(align - 1).expect("alignment overflow")) & !(align - 1)
    }

    fn assert_valid(&self, range: &core::ops::Range<usize>, label: &str) {
        if (range.start >> 32) != 0 || (range.end >> 32) != 0 {
            panic!(
                "{} reserved range carries high bits in low-vaddr build start=0x{start:016x} end=0x{end:016x}",
                label,
                start = range.start,
                end = range.end,
            );
        }
        assert!(
            range.start < range.end,
            "{} reserved range must be non-empty",
            label
        );
    }
}

/// Simple bump allocator for CSpace slots rooted at the initial thread's CNode.
pub struct SlotAllocator {
    cnode: seL4_CNode,
    start: seL4_CPtr,
    next: seL4_CPtr,
    end: seL4_CPtr,
    cnode_size_bits: seL4_Word,
    reserved_slots: ReservedSlotBitmap,
}

const ROOT_CSPACE_SLOT_CAPACITY: usize = 1 << 14;
const RESERVED_SLOT_WORD_BITS: usize = u64::BITS as usize;
const RESERVED_SLOT_WORDS: usize = ROOT_CSPACE_SLOT_CAPACITY / RESERVED_SLOT_WORD_BITS;

/// Fixed, allocation-free exact reservation set for the complete root CNode.
struct ReservedSlotBitmap {
    words: [u64; RESERVED_SLOT_WORDS],
}

impl ReservedSlotBitmap {
    const fn new() -> Self {
        Self {
            words: [0; RESERVED_SLOT_WORDS],
        }
    }

    fn contains(&self, slot: seL4_CPtr) -> bool {
        let Ok(index) = usize::try_from(slot) else {
            return false;
        };
        let Some(word) = self.words.get(index / RESERVED_SLOT_WORD_BITS) else {
            return false;
        };
        let bit = index % RESERVED_SLOT_WORD_BITS;
        (*word & (1u64 << bit)) != 0
    }

    fn insert(&mut self, slot: seL4_CPtr) -> Result<(), seL4_Error> {
        let index = usize::try_from(slot).map_err(|_| seL4_RangeError)?;
        let word = self
            .words
            .get_mut(index / RESERVED_SLOT_WORD_BITS)
            .ok_or(seL4_RangeError)?;
        let mask = 1u64 << (index % RESERVED_SLOT_WORD_BITS);
        if (*word & mask) != 0 {
            return Err(sel4_sys::seL4_DeleteFirst);
        }
        *word |= mask;
        Ok(())
    }
}

/// Snapshot describing the init CNode empty-slot window.
#[derive(Copy, Clone, Debug)]
pub struct SlotWindow {
    pub start: seL4_CPtr,
    pub next: seL4_CPtr,
    pub end: seL4_CPtr,
}

impl SlotAllocator {
    /// Creates a new allocator spanning the provided bootinfo slot region for the supplied root
    /// CNode capability.
    pub fn new(cnode: seL4_CNode, region: seL4_SlotRegion, cnode_size_bits: seL4_Word) -> Self {
        let capacity = 1usize
            .checked_shl(cnode_size_bits as u32)
            .unwrap_or(usize::MAX);
        debug_assert!(
            (region.end as usize) <= capacity,
            "bootinfo empty region exceeds root cnode capacity (end={:#x}, capacity={:#x}, bits={})",
            region.end,
            capacity,
            cnode_size_bits
        );
        Self {
            cnode,
            start: region.start,
            next: region.start,
            end: region.end,
            cnode_size_bits,
            reserved_slots: ReservedSlotBitmap::new(),
        }
    }

    /// Returns the number of free slots remaining in the allocator.
    #[must_use]
    pub fn remaining(&self) -> usize {
        (self.end - self.next) as usize
    }

    /// Returns the total capacity of the allocator in slots.
    #[must_use]
    pub fn capacity(&self) -> usize {
        (self.end - self.start) as usize
    }

    /// Returns the number of slots that have already been handed out.
    #[must_use]
    pub fn used(&self) -> usize {
        self.capacity().saturating_sub(self.remaining())
    }

    /// Returns a snapshot of the underlying bootinfo empty-slot window.
    #[must_use]
    pub fn window(&self) -> SlotWindow {
        SlotWindow {
            start: self.start,
            next: self.next,
            end: self.end,
        }
    }

    fn alloc(&mut self) -> Option<seL4_CPtr> {
        if self.next < self.start {
            ::log::warn!(
                "[cspace] next slot 0x{next:04x} before window start 0x{start:04x}; correcting",
                next = self.next,
                start = self.start,
            );
            self.next = self.start;
        }
        while self.next < self.end {
            while self.next < self.end
                && (is_boot_reserved_slot(self.next) || self.reserved_slots.contains(self.next))
            {
                self.next += 1;
            }
            if self.next >= self.end {
                break;
            }

            let slot = self.next;
            self.next += 1;
            let capacity = 1usize
                .checked_shl(self.cnode_size_bits as u32)
                .unwrap_or(usize::MAX);
            debug_assert!(
                (slot as usize) < capacity,
                "allocated cspace slot exceeds root cnode capacity",
            );

            if debug_cap_identify(slot) != 0 {
                ::log::warn!("[cspace] skipping occupied slot=0x{slot:04x}");
                continue;
            }

            return Some(slot);
        }

        None
    }

    /// Attempt to allocate a slot without panicking when the window is exhausted.
    #[must_use]
    pub fn try_alloc(&mut self) -> Option<seL4_CPtr> {
        self.alloc()
    }

    /// Excludes one exact empty slot from bump allocation for a retained anchor.
    pub fn reserve_exact(&mut self, slot: seL4_CPtr) -> Result<(), seL4_Error> {
        if slot < self.next
            || slot < self.start
            || slot >= self.end
            || is_boot_reserved_slot(slot)
            || self.reserved_slots.contains(slot)
            || debug_cap_identify(slot) != 0
        {
            return Err(sel4_sys::seL4_DeleteFirst);
        }
        self.reserved_slots.insert(slot)
    }

    /// Marks the first `slots` entries in the bootinfo empty window as consumed.
    pub fn consume_prefix(&mut self, slots: seL4_CPtr) {
        let new_next = self
            .start
            .checked_add(slots)
            .expect("cspace bootstrap consumption overflow");
        assert!(
            new_next <= self.end,
            "bootstrap slot consumption exceeds init CNode capacity"
        );
        if new_next > self.next {
            self.next = new_next;
        }
    }

    /// Returns the root CNode capability backing allocations.
    pub fn root(&self) -> seL4_CNode {
        self.cnode
    }

    /// Returns the radix depth (in bits) of the root CNode capability.
    ///
    /// For the init thread's single-level CSpace this equals `seL4_WordBits` because the kernel
    /// consumes the supplied root capability directly and addresses slots using the full word
    /// width.
    #[inline(always)]
    pub fn depth(&self) -> seL4_Word {
        sel4_sys::seL4_WordBits as seL4_Word
    }

    /// Returns the number of bits describing the capacity of the root CNode.
    ///
    /// This mirrors `bootinfo.initThreadCNodeSizeBits` and reflects how many slots are
    /// addressable within the initial CSpace root.
    #[inline(always)]
    pub fn capacity_bits(&self) -> seL4_Word {
        self.cnode_size_bits
    }
}

/// Returns `true` when the supplied slot index references a kernel-reserved capability.
///
/// The set mirrors Table 9.1 of the seL4 reference manual (version 16.0.0) and includes the
/// optional `seL4_CapSMC` slot provided by Arm kernels.
#[inline(always)]
#[allow(non_upper_case_globals)]
pub fn is_boot_reserved_slot(slot: seL4_CPtr) -> bool {
    if matches!(
        slot,
        seL4_CapNull
            | seL4_CapInitThreadTCB
            | seL4_CapInitThreadCNode
            | seL4_CapInitThreadVSpace
            | seL4_CapIRQControl
            | seL4_CapASIDControl
            | seL4_CapInitThreadASIDPool
            | seL4_CapIOPort
            | seL4_CapIOSpace
            | seL4_CapBootInfoFrame
            | seL4_CapInitThreadIPCBuffer
            | seL4_CapDomain
            | seL4_CapSMMUSIDControl
            | seL4_CapSMMUCBControl
            | seL4_CapInitThreadSC
            | seL4_CapSMC
    ) {
        return true;
    }
    if let Some(alias_slot) = canonical_root_alias_slot() {
        if alias_slot == slot {
            return true;
        }
    }
    false
}

/// Handle to an untyped capability reserved from the bootinfo catalog.
#[derive(Debug, PartialEq, Eq)]
pub struct ReservedUntyped {
    cap: seL4_Untyped,
    paddr: usize,
    previous_used_bytes: u128,
    offset_bytes: u128,
    size_bits: u8,
    index: usize,
    reserved_bytes: u128,
}

impl ReservedUntyped {
    /// Returns the capability slot referencing the reserved untyped.
    #[must_use]
    pub fn cap(&self) -> seL4_Untyped {
        self.cap
    }

    /// Returns the physical address backing the untyped capability.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Returns the offset in bytes from the start of the untyped region.
    #[must_use]
    pub fn offset_bytes(&self) -> u128 {
        self.offset_bytes
    }

    /// Returns the size of the reserved region in bits.
    #[must_use]
    pub fn size_bits(&self) -> u8 {
        self.size_bits
    }

    /// Returns the number of bytes reserved from this untyped instance.
    #[must_use]
    pub fn reserved_bytes(&self) -> u128 {
        self.reserved_bytes
    }

    /// Returns the index within the bootinfo untyped list.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }
}

/// Summary of untyped capability utilisation available to the root task.
#[derive(Copy, Clone, Debug)]
pub struct UntypedStats {
    /// Total number of untyped capabilities exported by the kernel.
    pub total: usize,
    /// Number of untyped capabilities that have been reserved so far.
    pub used: usize,
    /// Number of device-tagged untyped capabilities.
    pub device_total: usize,
    /// Number of device-tagged untyped capabilities that have been consumed.
    pub device_used: usize,
}

/// Diagnostic view describing a device untyped region that covers a physical range.
#[derive(Copy, Clone, Debug)]
pub struct DeviceCoverage {
    /// Physical base address of the underlying untyped region.
    pub base: usize,
    /// Exclusive upper bound of the untyped region.
    pub limit: usize,
    /// Size of the untyped region in bits.
    pub size_bits: u8,
    /// Index of the region within the bootinfo untyped list.
    pub index: usize,
    /// Indicates whether the region has already been reserved.
    pub used: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct DevicePtPool {
    ut_slot: seL4_CPtr,
    paddr: usize,
    size_bits: u8,
    index: usize,
    used_bytes: usize,
    total_bytes: usize,
}

impl DevicePtPool {
    pub fn from_config(config: DevicePtPoolConfig) -> Self {
        debug_assert!(
            config.size_bits <= (usize::BITS.saturating_sub(1) as u8),
            "device pt pool size_bits exceeds host word width",
        );
        let expected_bytes = 1usize
            .checked_shl(u32::from(config.size_bits))
            .expect("device pt pool size_bits overflowed host word width");
        Self {
            ut_slot: config.ut_slot,
            paddr: config.paddr,
            size_bits: config.size_bits,
            index: config.index,
            used_bytes: 0,
            total_bytes: expected_bytes,
        }
    }

    #[inline(always)]
    fn matches_index(&self, index: usize) -> bool {
        self.index == index
    }

    #[inline(always)]
    fn page_table_bytes(&self) -> usize {
        1usize << PAGE_TABLE_BITS
    }

    #[inline(always)]
    fn remaining_bytes(&self) -> usize {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    #[inline(always)]
    fn remaining_tables(&self) -> usize {
        self.remaining_bytes() / self.page_table_bytes()
    }

    fn reserve_page_table(&mut self) -> Result<ReservedUntyped, seL4_Error> {
        let page_table_bytes = self.page_table_bytes();
        let previous_used_bytes = self.used_bytes;
        let aligned_start =
            (self.used_bytes + (page_table_bytes - 1)) & !(page_table_bytes.saturating_sub(1));
        let end = aligned_start.saturating_add(page_table_bytes);
        let free_bytes = self.remaining_bytes();
        if end > self.total_bytes || page_table_bytes > free_bytes {
            log::error!(
                "[device-pt] pool insufficient: wanted {wanted}B but only {free}B free in ut=0x{ut:03x}",
                wanted = page_table_bytes,
                free = free_bytes,
                ut = self.ut_slot,
            );
            return Err(seL4_NotEnoughMemory);
        }
        self.used_bytes = end;
        log::trace!(
            "[device-pt] reserve ut=0x{ut:03x} paddr=0x{paddr:08x} used={used}B remaining_tables={remaining}",
            ut = self.ut_slot,
            paddr = self.paddr.saturating_add(aligned_start),
            used = self.used_bytes,
            remaining = self.remaining_tables(),
        );
        Ok(ReservedUntyped {
            cap: self.ut_slot,
            paddr: self.paddr.saturating_add(aligned_start),
            previous_used_bytes: previous_used_bytes as u128,
            offset_bytes: aligned_start as u128,
            size_bits: self.size_bits,
            index: self.index,
            reserved_bytes: page_table_bytes as u128,
        })
    }

    fn release(&mut self, reserved: &ReservedUntyped) {
        let expected_end = reserved
            .offset_bytes
            .saturating_add(reserved.reserved_bytes);
        if self.matches_index(reserved.index)
            && self.used_bytes as u128 == expected_end
            && reserved.previous_used_bytes <= reserved.offset_bytes
        {
            self.used_bytes = reserved.previous_used_bytes as usize;
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct TrackedUntyped {
    desc: UntypedDesc,
    used_bytes: u128,
}

impl TrackedUntyped {
    #[inline(always)]
    fn capacity_bytes(&self) -> u128 {
        1u128 << self.desc.size_bits
    }

    #[inline(always)]
    fn remaining_bytes(&self) -> u128 {
        self.capacity_bytes().saturating_sub(self.used_bytes)
    }
}

/// Index of bootinfo-provided untyped capabilities available to the root task.
pub struct UntypedCatalog<'a> {
    bootinfo: &'a seL4_BootInfo,
    entries: Vec<TrackedUntyped, MAX_BOOTINFO_UNTYPEDS>,
    device_pt_pool_index: Option<usize>,
}

impl<'a> UntypedCatalog<'a> {
    /// Creates a catalog view over the untyped list exported by seL4.
    pub fn new(bootinfo: &'a seL4_BootInfo, device_pt_pool_index: Option<usize>) -> Self {
        let count = bootinfo.untyped.end - bootinfo.untyped.start;
        let mut entries = Vec::new();
        for desc in &bootinfo.untypedList[..count as usize] {
            let tracked = TrackedUntyped {
                desc: (*desc).into(),
                used_bytes: 0,
            };
            entries
                .push(tracked)
                .expect("bootinfo untyped list exceeds MAX_BOOTINFO_UNTYPEDS");
        }
        Self {
            bootinfo,
            entries,
            device_pt_pool_index,
        }
    }

    fn reserve_index(&mut self, index: usize, obj_bits: u8) -> Option<ReservedUntyped> {
        let entry = self.entries.get_mut(index)?;
        let obj_bytes = Self::object_bytes(obj_bits);
        let capacity_bytes = entry.capacity_bytes();
        let previous_used_bytes = entry.used_bytes;
        let aligned_start = Self::aligned_start(entry.used_bytes, obj_bytes);
        let end = aligned_start.saturating_add(obj_bytes);
        if end > capacity_bytes {
            return None;
        }
        entry.used_bytes = end;
        Some(ReservedUntyped {
            cap: self.bootinfo.untyped.start + index as seL4_CPtr,
            paddr: entry.desc.paddr as usize + aligned_start as usize,
            previous_used_bytes,
            offset_bytes: aligned_start,
            size_bits: entry.desc.size_bits,
            index,
            reserved_bytes: obj_bytes,
        })
    }

    #[inline(always)]
    fn object_bytes(obj_bits: u8) -> u128 {
        1u128 << core::cmp::min(obj_bits, 127)
    }

    #[inline(always)]
    fn aligned_start(used_bytes: u128, obj_bytes: u128) -> u128 {
        (used_bytes + (obj_bytes - 1)) & !(obj_bytes - 1)
    }

    #[inline(always)]
    fn aligned_start_for_index(&self, index: usize, obj_bits: u8) -> Option<u128> {
        let entry = self.entries.get(index)?;
        let obj_bytes = Self::object_bytes(obj_bits);
        Some(Self::aligned_start(entry.used_bytes, obj_bytes))
    }

    #[inline(always)]
    fn entry_limit(entry: &TrackedUntyped) -> usize {
        let base = entry.desc.paddr as usize;
        base.saturating_add(1usize << entry.desc.size_bits)
    }

    fn best_device_index_for_range(
        &self,
        paddr: usize,
        size_bits: usize,
        obj_bits: u8,
    ) -> Option<usize> {
        let size_bytes = 1usize.checked_shl(size_bits as u32)?;
        let end = paddr.checked_add(size_bytes)?;
        let obj_bytes = Self::object_bytes(obj_bits);
        let mut best: Option<(usize, usize, usize, usize)> = None;
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.desc.is_device == 0 {
                continue;
            }
            let base = entry.desc.paddr as usize;
            let limit = Self::entry_limit(entry);
            if !(base <= paddr && end <= limit) {
                continue;
            }
            let aligned_start = Self::aligned_start(entry.used_bytes, obj_bytes);
            let aligned_start_usize = usize::try_from(aligned_start).ok()?;
            let next_paddr = base.checked_add(aligned_start_usize)?;
            if next_paddr > paddr {
                continue;
            }
            let aligned_end = aligned_start.saturating_add(obj_bytes);
            if aligned_end > entry.capacity_bytes() {
                continue;
            }
            let gap = paddr.saturating_sub(next_paddr);
            let span = limit.saturating_sub(base);
            match best {
                Some((best_gap, best_span, best_base, _))
                    if gap > best_gap
                        || (gap == best_gap && span > best_span)
                        || (gap == best_gap && span == best_span && base < best_base) => {}
                _ => best = Some((gap, span, base, index)),
            }
        }
        best.map(|(_, _, _, index)| index)
    }

    fn reserve_device_from_index(&mut self, index: usize, obj_bits: u8) -> Option<ReservedUntyped> {
        self.reserve_index(index, obj_bits)
    }

    /// Reserves an untyped covering the supplied device physical address range.
    pub fn reserve_device(&mut self, paddr: usize, size_bits: usize) -> Option<ReservedUntyped> {
        let obj_bits = size_bits as u8;
        let index = self.best_device_index_for_range(paddr, size_bits, obj_bits)?;
        if let Some(reserved) = self.reserve_device_from_index(index, obj_bits) {
            return Some(reserved);
        }
        log::error!(
            "[device-pt] device ut=0x{cap:03x} exhausted; skipping retype request",
            cap = self.bootinfo.untyped.start + index as seL4_CPtr,
        );
        None
    }

    /// Reserves the first RAM untyped meeting the requested size.
    pub fn reserve_ram(&mut self, obj_bits: u8) -> Option<ReservedUntyped> {
        let obj_bytes = 1u128 << core::cmp::min(obj_bits, 127);
        for index in 0..self.entries.len() {
            let should_reserve = {
                let entry = &self.entries[index];
                if self.device_pt_pool_index == Some(index)
                    || entry.desc.is_device != 0
                    || entry.desc.size_bits < obj_bits
                {
                    false
                } else if entry.remaining_bytes() < obj_bytes {
                    // Keep allocator selection side-effect free during runtime
                    // bring-up paths. Emitting logger-backed traces from this
                    // tight loop can deadlock when called under active console
                    // logging.
                    false
                } else {
                    true
                }
            };

            if should_reserve {
                if let Some(reserved) = self.reserve_index(index, obj_bits) {
                    return Some(reserved);
                }
            }
        }

        None
    }

    /// Reserves the highest-address RAM untyped meeting the requested size.
    pub fn reserve_ram_high(&mut self, obj_bits: u8) -> Option<ReservedUntyped> {
        let obj_bytes = 1u128 << core::cmp::min(obj_bits, 127);
        let mut best_index: Option<usize> = None;
        let mut best_end: usize = 0;
        for (index, entry) in self.entries.iter().enumerate() {
            if self.device_pt_pool_index == Some(index)
                || entry.desc.is_device != 0
                || entry.desc.size_bits < obj_bits
            {
                continue;
            }
            if entry.remaining_bytes() < obj_bytes {
                continue;
            }
            let base = entry.desc.paddr as usize;
            let end = base.saturating_add(1usize << entry.desc.size_bits);
            if best_index.is_none() || end > best_end {
                best_index = Some(index);
                best_end = end;
            }
        }
        if let Some(index) = best_index {
            return self.reserve_index(index, obj_bits);
        }
        None
    }

    /// Marks a physical address range as consumed within RAM untypeds.
    pub fn reserve_paddr_range(&mut self, range: Range<usize>, label: &'static str) {
        if range.start >= range.end {
            return;
        }
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if self.device_pt_pool_index == Some(index) || entry.desc.is_device != 0 {
                continue;
            }
            let base = entry.desc.paddr as usize;
            let end = base.saturating_add(1usize << entry.desc.size_bits);
            if range.end <= base || range.start >= end {
                continue;
            }
            let overlap_start = core::cmp::max(base, range.start);
            let overlap_end = core::cmp::min(end, range.end);
            let cap = entry.capacity_bytes();
            if overlap_start > base {
                log::warn!(
                    "[untyped] reserved range {label} overlaps ut=0x{cap:03x} mid-span; disabling entry base=0x{base:08x} end=0x{end:08x}",
                    cap = self.bootinfo.untyped.start + index as seL4_CPtr,
                    base = base,
                    end = end,
                );
                entry.used_bytes = cap;
                continue;
            }
            let used_bytes = overlap_end.saturating_sub(base) as u128;
            if used_bytes == 0 {
                continue;
            }
            let clamped = core::cmp::min(cap, used_bytes);
            log::info!(
                "[untyped] reserving {label} range in ut=0x{cap:03x} base=0x{base:08x} used_bytes=0x{used:x}",
                cap = self.bootinfo.untyped.start + index as seL4_CPtr,
                base = base,
                used = clamped,
            );
            entry.used_bytes = core::cmp::max(entry.used_bytes, clamped);
        }
    }

    /// Rolls back the newest reservation before any successful retype consumes it.
    ///
    /// A non-LIFO or already-consumed token is left reserved. This fail-closed
    /// behavior keeps the software cursor from moving behind seL4's untyped
    /// watermark.
    pub fn release(&mut self, reserved: &ReservedUntyped) {
        if let Some(entry) = self.entries.get_mut(reserved.index) {
            let expected_end = reserved
                .offset_bytes
                .saturating_add(reserved.reserved_bytes);
            let expected_cap = self.bootinfo.untyped.start + reserved.index as seL4_CPtr;
            if reserved.cap == expected_cap
                && entry.used_bytes == expected_end
                && reserved.previous_used_bytes <= reserved.offset_bytes
            {
                entry.used_bytes = reserved.previous_used_bytes;
            }
        }
    }

    /// Returns diagnostic statistics describing untyped catalogue utilisation.
    #[must_use]
    pub fn stats(&self) -> UntypedStats {
        let total = self.entries.len();
        let used = self
            .entries
            .iter()
            .filter(|entry| entry.used_bytes > 0)
            .count();
        let device_total = self
            .entries
            .iter()
            .filter(|entry| entry.desc.is_device != 0)
            .count();
        let device_used = self
            .entries
            .iter()
            .filter(|entry| entry.desc.is_device != 0 && entry.used_bytes > 0)
            .count();
        UntypedStats {
            total,
            used,
            device_total,
            device_used,
        }
    }

    /// Records previously consumed bytes for the specified untyped index.
    pub fn record_usage(&mut self, index: usize, used_bytes: u128) {
        if let Some(entry) = self.entries.get_mut(index) {
            let clamped = core::cmp::min(entry.capacity_bytes(), used_bytes);
            entry.used_bytes = core::cmp::max(entry.used_bytes, clamped);
        }
    }

    /// Locates the device untyped covering the requested physical range, if available.
    #[must_use]
    pub fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        let index = self.best_device_index_for_range(paddr, size_bits, PAGE_BITS as u8)?;
        let entry = self.entries.get(index)?;
        let base = entry.desc.paddr as usize;
        let limit = Self::entry_limit(entry);
        Some(DeviceCoverage {
            base,
            limit,
            size_bits: entry.desc.size_bits,
            index,
            used: entry.used_bytes > 0,
        })
    }
}

/// Virtual mapping of a physical device frame.
#[derive(Clone)]
pub struct DeviceFrame {
    cap: seL4_CPtr,
    paddr: usize,
    ptr: NonNull<u8>,
}

impl DeviceFrame {
    /// Returns the capability referencing this frame.
    #[must_use]
    pub fn cap(&self) -> seL4_CPtr {
        self.cap
    }

    /// Returns the virtual pointer to the mapped frame.
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    /// Returns the physical address backing the device frame.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Builds a bounded dummy device frame for host-side unit tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test(ptr: NonNull<u8>, paddr: usize) -> Self {
        Self { cap: 0, paddr, ptr }
    }
}

#[derive(Copy, Clone)]
struct DeviceFrameCacheEntry {
    paddr: usize,
    source_cap: seL4_CPtr,
    root_cap: Option<seL4_CPtr>,
    root_vaddr: Option<usize>,
    exclusive_child_admission: bool,
}

const ROOT_DEVICE_CACHE_BCM2711_BUS_START: usize = 0x7e00_0000;
const ROOT_DEVICE_CACHE_BCM2711_BUS_END: usize = 0x8000_0000;
const ROOT_DEVICE_CACHE_BCM2711_PERIPH_START: usize = 0xfd00_0000;
const ROOT_DEVICE_CACHE_BCM2711_PERIPH_END: usize = 0xff00_0000;
const ROOT_DEVICE_CACHE_VL805_BAR_START: usize = 0x0000_0006_0000_0000;
const ROOT_DEVICE_CACHE_VL805_BAR_END: usize = 0x0000_0006_0010_0000;

fn root_device_frame_cache_eligible(paddr: usize) -> bool {
    (ROOT_DEVICE_CACHE_BCM2711_BUS_START..ROOT_DEVICE_CACHE_BCM2711_BUS_END).contains(&paddr)
        || (ROOT_DEVICE_CACHE_BCM2711_PERIPH_START..ROOT_DEVICE_CACHE_BCM2711_PERIPH_END)
            .contains(&paddr)
        || (ROOT_DEVICE_CACHE_VL805_BAR_START..ROOT_DEVICE_CACHE_VL805_BAR_END).contains(&paddr)
}

/// Virtual mapping of DMA-capable RAM used for driver buffers.
#[derive(Clone)]
pub struct RamFrame {
    cap: seL4_CPtr,
    paddr: usize,
    ptr: NonNull<u8>,
}

impl RamFrame {
    /// Returns the virtual pointer to the mapped RAM.
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    /// Returns the physical address for DMA.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Returns the capability referencing this RAM frame.
    #[must_use]
    pub fn cap(&self) -> seL4_CPtr {
        self.cap
    }

    /// Builds a bounded dummy RAM frame for host-side unit tests.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn for_test(ptr: NonNull<u8>, paddr: usize) -> Self {
        Self { cap: 0, paddr, ptr }
    }

    /// Returns the frame as a mutable byte slice covering one page.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), PAGE_SIZE) }
    }

    /// Returns the frame as an immutable byte slice covering one page.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), PAGE_SIZE) }
    }
}

/// RAM frame capability intentionally left unmapped in the root VSpace.
#[derive(Clone)]
pub struct UnmappedRamFrame {
    cap: seL4_CPtr,
    paddr: usize,
}

impl UnmappedRamFrame {
    /// Returns the physical address for device-owned DMA.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Returns the capability referencing this RAM frame.
    #[must_use]
    pub fn cap(&self) -> seL4_CPtr {
        self.cap
    }
}

/// Aggregates bootinfo-derived allocators and helpers for the root task.
pub struct KernelEnv<'a> {
    bootinfo: &'a seL4_BootInfo,
    slots: SlotAllocator,
    untyped: UntypedCatalog<'a>,
    page_tables: PageTableBookkeeper<MAX_PAGE_TABLES>,
    page_directories: PageDirectoryBookkeeper<MAX_PAGE_DIRECTORIES>,
    page_upper_directories: PageUpperDirectoryBookkeeper<MAX_PAGE_UPPER_DIRECTORIES>,
    device_cursor: usize,
    dma_cursor: usize,
    last_retype: Option<RetypeLog>,
    ipcbuf_trace: bool,
    ipcbuf_view: Option<IpcBufView>,
    device_skip_objects: Vec<seL4_CPtr, MAX_DEVICE_SKIP_OBJECTS>,
    device_frame_cache: Vec<DeviceFrameCacheEntry, MAX_DEVICE_FRAME_CACHE>,
    device_pt_pool: Option<DevicePtPool>,
    reserved: ReservedVaddrRanges,
}

/// Diagnostic snapshot capturing resource utilisation within the [`KernelEnv`].
#[derive(Copy, Clone, Debug)]
pub struct KernelEnvSnapshot {
    /// Virtual base of the device-mapping window.
    pub device_base: usize,
    /// Virtual cursor indicating the next free device mapping address.
    pub device_cursor: usize,
    /// Virtual base of the DMA window.
    pub dma_base: usize,
    /// Virtual cursor indicating the next free DMA mapping address.
    pub dma_cursor: usize,
    /// Capability designating the root CNode supplied to retype operations.
    pub cspace_root: seL4_CNode,
    /// Traversal depth (in bits) used when submitting CSpace paths (equals `seL4_WordBits`).
    pub cspace_root_depth: seL4_Word,
    /// Total number of CSpace slots managed by the allocator.
    pub cspace_capacity: usize,
    /// Number of CSpace slots handed out so far.
    pub cspace_used: usize,
    /// Number of CSpace slots remaining for future allocations.
    pub cspace_remaining: usize,
    /// Number of level-3 page tables currently mapped into the VSpace.
    pub page_tables_mapped: usize,
    /// Number of level-2 page directories currently mapped into the VSpace.
    pub page_directories_mapped: usize,
    /// Number of level-1 page upper directories currently mapped into the VSpace.
    pub page_upper_directories_mapped: usize,
    /// Summary of untyped catalogue utilisation.
    pub untyped: UntypedStats,
    /// Last observed retype attempt emitted by the environment.
    pub last_retype: Option<RetypeLog>,
}

/// Classification of the object that was being created during a retype attempt.
#[derive(Copy, Clone, Debug)]
pub enum RetypeKind {
    /// Device-mapped frame for MMIO peripherals.
    DevicePage {
        /// Physical base address of the targeted MMIO frame.
        paddr: usize,
    },
    /// DMA-capable RAM frame allocated for drivers.
    DmaPage {
        /// Physical base address of the RAM frame being retyped.
        paddr: usize,
    },
    /// Page table backing a virtual mapping.
    PageTable {
        /// Virtual base address of the page table's mapping range.
        vaddr: usize,
    },
    /// Page directory covering a 1 GiB region in the VSpace.
    PageDirectory {
        /// Virtual base address of the page directory's mapping range.
        vaddr: usize,
    },
    /// Page upper directory covering a 512 GiB region in the VSpace.
    PageUpperDirectory {
        /// Virtual base address of the page upper directory's mapping range.
        vaddr: usize,
    },
    /// A root VSpace object that will receive an ASID before driver use.
    VSpaceRoot,
}

/// Detailed snapshot of the parameters used for a `seL4_Untyped_Retype` call.
///
/// The destination root **must** be the writable init thread CNode capability resident in slot
/// `seL4_CapInitThreadCNode`. Do not use allocator handles or read-only aliases. The init CSpace is
/// single-level, so the kernel resolves the init CNode capability by direct CSpace addressing:
/// `node_depth = seL4_WordBits`, `node_index = seL4_CapInitThreadCNode`, and
/// `dest_offset = dest_slot`.
#[derive(Copy, Clone, Debug)]
pub struct RetypeTrace {
    /// Capability designating the source untyped region.
    pub untyped_cap: seL4_Untyped,
    /// Physical base address advertised by the untyped descriptor.
    pub untyped_paddr: usize,
    /// Size (in bits) of the backing untyped region.
    pub untyped_size_bits: u8,
    /// Capability designating the root CNode supplied to the kernel.
    pub cnode_root: seL4_CNode,
    /// Destination slot selected for the newly created object.
    pub dest_slot: seL4_CPtr,
    /// Slot offset resolved relative to `cnode_root`.
    /// Root CNode policy for this system: `dest_offset = dest_slot`.
    pub dest_offset: seL4_Word,
    /// `nodeDepth` argument supplied to `seL4_Untyped_Retype` while resolving the destination CNode.
    /// Root CNode policy for this system: `cnode_depth = seL4_WordBits`.
    pub cnode_depth: seL4_Word,
    /// `nodeIndex` argument supplied to `seL4_Untyped_Retype` when selecting a sub-CNode below
    /// `cnode_root`. Root CNode policy for this system: `node_index = seL4_CapInitThreadCNode`.
    pub node_index: seL4_Word,
    /// Object type requested from the kernel.
    pub object_type: seL4_Word,
    /// Object size (in bits) supplied to the kernel.
    pub object_size_bits: seL4_Word,
    /// High-level description of the object being materialised.
    pub kind: RetypeKind,
}

/// Result marker describing whether the most recent retype succeeded.
#[derive(Copy, Clone, Debug)]
pub enum RetypeStatus {
    /// A retype call has not yet completed.
    Pending,
    /// The retype call completed successfully.
    Ok,
    /// The retype call failed with the captured error code.
    Err(seL4_Error),
}

/// Detailed reason describing why a retype trace could not be sanitised for kernel submission.
#[derive(Copy, Clone, Debug)]
pub enum RetypeSanitiseError {
    /// The supplied CNode capability did not match the writable init thread root CNode.
    RootMismatch {
        /// Capability provided by the caller.
        provided: seL4_CNode,
        /// Capability expected by the root-task allocator.
        expected: seL4_CNode,
    },
    /// The depth did not match direct init-CNode addressing for the init CSpace.
    DepthMismatch {
        /// Depth supplied in the trace.
        provided: seL4_Word,
        /// Expected depth derived from bootinfo.
        expected: seL4_Word,
    },
    /// The node index exceeded the writable init thread CNode capacity.
    NodeIndexOutOfRange {
        /// Node index supplied in the trace.
        provided: seL4_Word,
        /// Maximum representable slot index for the init CNode.
        capacity: usize,
    },
    /// The node index did not match the init thread root CNode capability slot.
    NodeIndexMismatch {
        /// Node index supplied in the trace.
        provided: seL4_Word,
        /// Expected traversal index when targeting the init thread root CNode.
        expected: seL4_Word,
    },
    /// The destination offset exceeded the init CNode's slot capacity.
    OffsetOutOfRange {
        /// Offset supplied in the trace.
        provided: seL4_Word,
        /// Maximum representable slot index for the init CNode.
        capacity: usize,
    },
    /// The destination offset and reported capability slot diverged.
    DestOffsetMismatch {
        /// Destination offset supplied in the trace.
        offset: seL4_Word,
        /// Canonical offset expected when targeting the init CSpace root (always zero).
        slot: seL4_Word,
    },
}

impl fmt::Display for RetypeSanitiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMismatch { provided, expected } => {
                write!(
                    f,
                    "root mismatch: provided=0x{provided:04x} expected=0x{expected:04x}"
                )
            }
            Self::DepthMismatch { provided, expected } => {
                write!(
                    f,
                    "cnode_depth mismatch: provided={} expected={}",
                    provided, expected
                )
            }
            Self::NodeIndexOutOfRange { provided, capacity } => {
                write!(
                    f,
                    "node_index out of range: provided=0x{provided:04x} capacity={capacity}",
                )
            }
            Self::NodeIndexMismatch { provided, expected } => {
                write!(
                    f,
                    "node_index mismatch: provided=0x{provided:04x} expected=0x{expected:04x}",
                )
            }
            Self::OffsetOutOfRange { provided, capacity } => {
                write!(
                    f,
                    "dest_offset out of range: provided=0x{provided:04x} capacity={capacity}",
                )
            }
            Self::DestOffsetMismatch { offset, slot } => {
                write!(
                    f,
                    "dest_offset/slot mismatch: offset=0x{offset:04x} slot=0x{slot:04x}",
                )
            }
        }
    }
}

/// Log entry capturing the trace and outcome for the latest retype attempt.
#[derive(Copy, Clone, Debug)]
pub struct RetypeLog {
    /// Parameters passed to the kernel.
    pub trace: RetypeTrace,
    /// Expected writable init thread CNode capability derived from bootinfo.
    pub init_cnode_cap: seL4_CNode,
    /// Slot index of the writable init thread CNode capability.
    pub init_cnode_slot: seL4_Word,
    /// Guard depth (in bits) advertised by bootinfo for the init CNode.
    pub init_cnode_bits: usize,
    /// Maximum number of slots available in the init thread CSpace root.
    pub init_cnode_capacity: usize,
    /// Kernel-advertised radix depth used when retyping into the init CSpace root.
    pub canonical_cnode_depth: seL4_Word,
    /// Sanitised trace prepared for submission to the kernel, if available.
    pub sanitised: Option<RetypeTrace>,
    /// Detailed reason explaining why sanitisation failed, if applicable.
    pub sanitise_error: Option<RetypeSanitiseError>,
    /// Outcome returned by the kernel.
    pub status: RetypeStatus,
}

impl<'a> KernelEnv<'a> {
    /// Builds a new environment from the seL4 bootinfo struct.
    pub fn new(
        bootinfo: &'a seL4_BootInfo,
        device_pt_pool: Option<DevicePtPool>,
        reserved: ReservedVaddrRanges,
    ) -> Self {
        let root_cnode_bits = bootinfo.init_cnode_bits();
        assert!(
            root_cnode_bits > 0,
            "BootInfo initBits is 0 — capacity invalid"
        );
        let capacity = 1usize
            .checked_shl(root_cnode_bits as u32)
            .unwrap_or_else(|| panic!("initBits {} exceeds host word size", root_cnode_bits));
        let empty_start = bootinfo.empty_first_slot();
        let empty_end = bootinfo.empty_last_slot_excl();
        let span = empty_end.saturating_sub(empty_start);
        log::info!(
            "[cohesix:root-task] bootinfo.empty slots [0x{start:04x}..0x{end:04x}) span={span} root_cnode_bits={bits}",
            start = empty_start,
            end = empty_end,
            span = span,
            bits = root_cnode_bits
        );
        assert!(
            empty_end <= capacity,
            "bootinfo empty region exceeds root cnode capacity (end={:#x}, capacity={:#x}, bits={})",
            empty_end,
            capacity,
            root_cnode_bits
        );

        let slots = SlotAllocator::new(
            bootinfo.init_cnode_cap(),
            bootinfo.empty,
            root_cnode_bits as seL4_Word,
        );
        BOOTINFO_WINDOW_GUARD.arm(bootinfo);
        let pool_index = device_pt_pool.as_ref().map(|pool| pool.index);
        if let Some(pool) = device_pt_pool.as_ref() {
            let remaining_tables = pool.remaining_tables();
            log::info!(
                "[device-pt] reserved pool ut=0x{ut:03x} tables={tables} bytes={bytes}",
                ut = pool.ut_slot,
                tables = remaining_tables,
                bytes = pool.remaining_bytes(),
            );
            assert!(
                remaining_tables > 0,
                "device page-table pool exhausted during bootstrap reservation"
            );
        }
        let mut untyped = UntypedCatalog::new(bootinfo, pool_index);
        if let Some(range) = user_image_paddr_range() {
            let mut line = HeaplessString::<160>::new();
            let _ = write!(
                &mut line,
                "[untyped] user-image paddr=[0x{start:08x}..0x{end:08x})",
                start = range.start,
                end = range.end,
            );
            boot_log::force_uart_line(line.as_str());
            log::info!("{}", line.as_str());
            untyped.reserve_paddr_range(range, "user-image");
        } else {
            log::warn!(
                "[untyped] user image paddr range unavailable; DMA allocations may overlap image"
            );
            boot_log::force_uart_line(
                "[untyped] user image paddr range unavailable; DMA allocations may overlap image",
            );
        }
        reserve_bootinfo_snapshot_paddrs(&mut untyped);
        untyped.reserve_paddr_range(0..DMA_LOW_GUARD_BYTES, "dma-low-guard");
        let mut guard_line = HeaplessString::<112>::new();
        let _ = write!(
            &mut guard_line,
            "[untyped] reserved dma-low-guard [0x00000000..0x{end:08x})",
            end = DMA_LOW_GUARD_BYTES,
        );
        boot_log::force_uart_line(guard_line.as_str());
        log::info!("{}", guard_line.as_str());
        Self {
            bootinfo,
            slots,
            untyped,
            page_tables: PageTableBookkeeper::new(),
            page_directories: PageDirectoryBookkeeper::new(),
            page_upper_directories: PageUpperDirectoryBookkeeper::new(),
            device_cursor: DEVICE_VADDR_BASE,
            dma_cursor: DMA_VADDR_BASE,
            last_retype: None,
            ipcbuf_trace: false,
            ipcbuf_view: None,
            device_skip_objects: Vec::new(),
            device_frame_cache: Vec::new(),
            device_pt_pool,
            reserved,
        }
    }

    /// Returns the bootinfo pointer passed to the root task.
    pub fn bootinfo(&self) -> &'a seL4_BootInfo {
        self.bootinfo
    }

    /// Records previously consumed bytes for a bootinfo-provided untyped.
    pub fn record_untyped_bytes(&mut self, index: usize, used_bytes: u128) {
        self.untyped.record_usage(index, used_bytes);
    }

    pub fn reserve_vaddr_range(&mut self, range: &core::ops::Range<usize>, label: &'static str) {
        self.reserved.reserve(range, label);
    }

    /// Returns a view over the init thread IPC buffer if it has been installed.
    pub fn ipc_buffer_view(&self) -> Option<IpcBufView> {
        self.ipcbuf_view
    }

    /// Records the boot-provided IPC buffer mapping for the init thread without
    /// invoking a TCB rebind.
    pub fn record_boot_ipc_buffer(&mut self, frame: seL4_CPtr, vaddr: usize) -> IpcBufView {
        debug_assert_ne!(vaddr, 0, "IPC buffer pointer must be non-null");
        let view = unsafe { IpcBufView::new(vaddr as *const u8, frame) };
        self.ipcbuf_view = Some(view);
        view
    }

    /// Marks a prefix of the bootinfo empty slot region as consumed by early bootstrap code.
    pub fn consume_bootstrap_slots(&mut self, slots: usize) {
        if slots == 0 {
            return;
        }
        let count: seL4_CPtr = slots
            .try_into()
            .expect("bootstrap slot count must fit in seL4_CPtr");
        self.slots.consume_prefix(count);
    }

    /// Returns the writable init CNode capability published through bootinfo.
    #[inline(always)]
    pub fn init_cnode_cap(&self) -> seL4_CNode {
        self.bootinfo.init_cnode_cap()
    }

    #[inline(always)]
    fn root_guard_depth(&self) -> seL4_Word {
        self.bootinfo.init_cnode_depth() as seL4_Word
    }

    /// Produces a diagnostic snapshot describing allocator state.
    #[must_use]
    pub fn snapshot(&self) -> KernelEnvSnapshot {
        let cspace_capacity = self.slots.capacity();
        let cspace_remaining = self.slots.remaining();
        KernelEnvSnapshot {
            device_base: DEVICE_VADDR_BASE,
            device_cursor: self.device_cursor,
            dma_base: DMA_VADDR_BASE,
            dma_cursor: self.dma_cursor,
            cspace_root: self.slots.root(),
            cspace_root_depth: self.slots.depth(),
            cspace_capacity,
            cspace_used: self.slots.used(),
            cspace_remaining,
            page_tables_mapped: self.page_tables.count(),
            page_directories_mapped: self.page_directories.count(),
            page_upper_directories_mapped: self.page_upper_directories.count(),
            untyped: self.untyped.stats(),
            last_retype: self.last_retype,
        }
    }

    /// Returns the device untyped covering the supplied range, if any, without reserving it.
    #[must_use]
    pub fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        self.untyped.device_coverage(paddr, size_bits)
    }

    /// Returns whether an admitted device page can be copied into a child VSpace.
    ///
    /// A page already retyped and cached by root no longer has fresh device-untyped
    /// coverage at its original address, but its cached source capability remains
    /// the authoritative HAL object for a one-way driver-runtime handoff.
    #[must_use]
    pub fn device_page_available_for_child(&self, paddr: usize) -> bool {
        self.cached_device_frame_for_paddr(paddr).is_some()
            || self.untyped.device_coverage(paddr, PAGE_BITS).is_some()
    }

    /// Returns whether HAL retained the page solely as an unmapped child
    /// admission capability.
    #[must_use]
    pub fn device_page_admitted_for_child_without_root_mapping(&self, paddr: usize) -> bool {
        self.cached_device_frame_for_paddr(paddr)
            .is_some_and(|entry| {
                entry.exclusive_child_admission
                    && entry.root_cap.is_none()
                    && entry.root_vaddr.is_none()
            })
    }

    /// Retypes and retains an unmapped HAL capability for a later child-VSpace map.
    ///
    /// seL4 device-untyped allocation is monotonic within an untyped. A later
    /// root mapping at a higher physical address can therefore make an earlier
    /// device page permanently unreachable. Runtime bootstrap uses this method
    /// to admit exact low MMIO pages in physical order without creating a root
    /// mapping or exposing a root-owned steady-state device path.
    pub fn admit_device_page_for_child(&mut self, paddr: usize) -> Result<seL4_CPtr, seL4_Error> {
        if let Some(cached) = self.cached_device_frame_for_paddr(paddr) {
            return if cached.exclusive_child_admission
                && cached.root_cap.is_none()
                && cached.root_vaddr.is_none()
            {
                Ok(cached.source_cap)
            } else {
                Err(sel4_sys::seL4_IllegalOperation)
            };
        }
        let frame_slot =
            self.retype_device_page_for_paddr(paddr, "driver-vspace-device-admission")?;
        self.remember_device_frame_cap(paddr, frame_slot, true)?;
        Ok(frame_slot)
    }

    fn dump_bootinfo_window_once(&self, label: &str) {
        if BOOTINFO_WINDOW_DUMPED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let empty = &self.bootinfo.empty;
        let dump_len = mem::size_of::<sel4_sys::seL4_SlotRegion>().min(64);
        let empty_bytes =
            unsafe { core::slice::from_raw_parts(empty as *const _ as *const u8, dump_len) };
        let mut line = HeaplessString::<256>::new();
        let _ = write!(
            &mut line,
            "[bootinfo.window] label={} start=0x{start:04x} end=0x{end:04x} bytes=",
            label,
            start = empty.start,
            end = empty.end
        );
        for byte in empty_bytes.iter() {
            if write!(&mut line, "{byte:02x}").is_err() {
                break;
            }
        }
        boot_log::force_uart_line(line.as_str());
    }

    /// Allocates a new CSpace slot, panicking if the root CNode is exhausted.
    pub fn allocate_slot(&mut self) -> seL4_CPtr {
        BOOTINFO_WINDOW_GUARD.check("allocate_slot");
        let slot = self
            .slots
            .alloc()
            .expect("cspace exhausted while allocating seL4 objects");
        let empty_start = self.bootinfo.empty.start;
        let empty_end = self.bootinfo.empty.end;
        let slot_valid = slot >= empty_start && slot < empty_end;
        if !slot_valid {
            self.dump_bootinfo_window_once("allocate_slot");
        }
        assert!(
            slot_valid,
            "allocated slot 0x{slot:04x} outside bootinfo window [0x{start:04x}..0x{end:04x})",
            start = empty_start,
            end = empty_end,
        );
        slot
    }

    /// Allocates a CSpace slot without panicking when admission is exhausted.
    pub fn try_allocate_slot(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        BOOTINFO_WINDOW_GUARD.check("try_allocate_slot");
        let slot = self.slots.try_alloc().ok_or(seL4_NotEnoughMemory)?;
        if slot < self.bootinfo.empty.start || slot >= self.bootinfo.empty.end {
            self.dump_bootinfo_window_once("try_allocate_slot");
            return Err(seL4_RangeError);
        }
        Ok(slot)
    }

    /// Reserves one compiler-selected empty init-CNode slot as a revoke anchor.
    pub fn reserve_cspace_anchor_slot(&mut self, slot: seL4_CPtr) -> Result<(), seL4_Error> {
        BOOTINFO_WINDOW_GUARD.check("reserve_cspace_anchor_slot");
        self.slots.reserve_exact(slot)
    }

    /// Retypes one dedicated child untyped into an exact compiler-reserved slot.
    ///
    /// The returned cap is a real revoke anchor: every object subsequently
    /// retyped from it is deleted by `CNode_Revoke(anchor)`, which also resets
    /// the child-untyped watermark for deterministic generation reuse. The
    /// parent BootInfo reservation remains consumed for the root lifetime.
    pub fn create_revoke_anchor(
        &mut self,
        anchor_slot: seL4_CPtr,
        size_bits: u8,
    ) -> Result<seL4_CPtr, seL4_Error> {
        if size_bits < PAGE_BITS as u8 || seL4_Word::from(size_bits) >= word_bits() {
            return Err(seL4_RangeError);
        }
        self.reserve_cspace_anchor_slot(anchor_slot)?;
        let reserved = self
            .untyped
            .reserve_ram(size_bits)
            .ok_or(seL4_NotEnoughMemory)?;
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_UntypedObject as seL4_Word,
            seL4_Word::from(size_bits),
            anchor_slot,
        ) {
            Ok(()) => Ok(anchor_slot),
            Err(error) => {
                self.untyped.release(&reserved);
                Err(error.into_sel4_error())
            }
        }
    }

    /// Retypes one generation object from a retained child-untyped anchor into
    /// an already reserved empty init-CNode slot.
    pub fn retype_from_revoke_anchor(
        &self,
        anchor: seL4_CPtr,
        object_type: seL4_Word,
        object_size_bits: seL4_Word,
        destination_slot: seL4_CPtr,
    ) -> Result<(), seL4_Error> {
        if anchor == seL4_CapNull || destination_slot == seL4_CapNull {
            return Err(sel4_sys::seL4_InvalidCapability);
        }
        cspace_sys::untyped_retype_into_init_root(
            anchor,
            object_type,
            object_size_bits,
            destination_slot,
        )
        .map_err(cspace_sys::RetypeCallError::into_sel4_error)
    }

    /// Maps one anchor-derived RAM frame into a fresh root-only virtual page.
    ///
    /// The caller may keep this alias for shared records or unmap/delete it
    /// after copying immutable child bytes. The frame capability remains a
    /// descendant of the supplied revoke anchor.
    pub fn map_revoke_anchor_frame_in_root(
        &mut self,
        frame_cap: seL4_CPtr,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, seL4_Error> {
        let range = self.next_mapping_range(self.dma_cursor, PAGE_SIZE, "revoke-anchor-frame");
        self.dma_cursor = range.end;
        self.map_frame(frame_cap, range.start, attr, true)?;
        let paddr = page_get_address(frame_cap)?;
        Ok(RamFrame {
            cap: frame_cap,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(range.start))
                .ok_or(seL4_RangeError)?,
        })
    }

    /// Maps one already rights-reduced anchor-derived frame cap into a fresh
    /// root-only virtual page with those exact rights.
    pub fn map_revoke_anchor_frame_in_root_with_rights(
        &mut self,
        frame_cap: seL4_CPtr,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, seL4_Error> {
        let range =
            self.next_mapping_range(self.dma_cursor, PAGE_SIZE, "revoke-anchor-frame-rights");
        self.dma_cursor = range.end;
        self.map_frame_with_rights(frame_cap, range.start, rights, attr, true)?;
        let paddr = page_get_address(frame_cap)?;
        Ok(RamFrame {
            cap: frame_cap,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(range.start))
                .ok_or(seL4_RangeError)?,
        })
    }

    /// Remaps a new anchor-derived frame into a previously established root
    /// scratch page. The prior generation must already be revoked, so no old
    /// frame mapping may remain at `vaddr`; existing root translation tables
    /// are reused without consuming another virtual range.
    pub fn remap_revoke_anchor_frame_in_root(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, seL4_Error> {
        if frame_cap == seL4_CapNull || vaddr == 0 || vaddr & (PAGE_SIZE - 1) != 0 {
            return Err(seL4_RangeError);
        }
        self.map_frame(frame_cap, vaddr, attr, true)?;
        let paddr = page_get_address(frame_cap)?;
        Ok(RamFrame {
            cap: frame_cap,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(vaddr))
                .ok_or(seL4_RangeError)?,
        })
    }

    /// Retypes and returns a notification object in the init CSpace.
    pub fn alloc_notification(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(sel4_sys::seL4_NotificationBits as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_NotificationObject as seL4_Word,
            sel4_sys::seL4_NotificationBits as seL4_Word,
            slot,
        ) {
            Ok(()) => Ok(slot),
            Err(err) => {
                self.untyped.release(&reserved);
                Err(err.into_sel4_error())
            }
        }
    }

    /// Retypes and returns an endpoint object in the init CSpace.
    pub fn alloc_endpoint(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(sel4_sys::seL4_EndpointBits as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_EndpointObject as seL4_Word,
            0,
            slot,
        ) {
            Ok(()) => Ok(slot),
            Err(err) => {
                self.untyped.release(&reserved);
                Err(err.into_sel4_error())
            }
        }
    }

    /// Retypes and returns a TCB object in the init CSpace.
    pub fn alloc_tcb(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(sel4_sys::seL4_TCBBits as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_TCBObject as seL4_Word,
            0,
            slot,
        ) {
            Ok(()) => Ok(slot),
            Err(err) => {
                self.untyped.release(&reserved);
                Err(err.into_sel4_error())
            }
        }
    }

    /// Retypes an MCS scheduling context from BootInfo-owned RAM authority.
    #[cfg(sel4_config_kernel_mcs)]
    pub fn alloc_sched_context(&mut self, object_bits: u8) -> Result<seL4_CPtr, seL4_Error> {
        let minimum =
            u8::try_from(sel4_sys::SEL4_MCS_MIN_SCHED_CONTEXT_BITS).map_err(|_| seL4_RangeError)?;
        if object_bits < minimum || u64::from(object_bits) >= sel4_sys::seL4_WordBits {
            return Err(seL4_RangeError);
        }
        let reserved = self
            .untyped
            .reserve_ram(object_bits)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_SchedContextObject as seL4_Word,
            seL4_Word::from(object_bits),
            slot,
        ) {
            Ok(()) => Ok(slot),
            Err(error) => {
                self.untyped.release(&reserved);
                Err(error.into_sel4_error())
            }
        }
    }

    /// Retypes one MCS Reply object from BootInfo-owned RAM authority.
    #[cfg(sel4_config_kernel_mcs)]
    pub fn alloc_reply(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let object_bits =
            u8::try_from(sel4_sys::SEL4_MCS_REPLY_BITS).map_err(|_| seL4_RangeError)?;
        let reserved = self
            .untyped
            .reserve_ram(object_bits)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_ReplyObject as seL4_Word,
            0,
            slot,
        ) {
            Ok(()) => Ok(slot),
            Err(error) => {
                self.untyped.release(&reserved);
                Err(error.into_sel4_error())
            }
        }
    }

    /// Returns the BootInfo-provided SchedControl cap for one exact core.
    #[cfg(sel4_config_kernel_mcs)]
    pub fn sched_control_for_core(&self, core: u8) -> Result<seL4_CPtr, seL4_Error> {
        if seL4_Word::from(core) >= self.bootinfo.numNodes {
            return Err(seL4_RangeError);
        }
        let cap = self
            .bootinfo
            .schedcontrol
            .start
            .checked_add(seL4_CPtr::from(core))
            .ok_or(seL4_RangeError)?;
        if cap >= self.bootinfo.schedcontrol.end {
            return Err(seL4_RangeError);
        }
        Ok(cap)
    }

    /// Retypes and returns a CNode object with `radix_bits` slots.
    pub fn alloc_cnode(&mut self, radix_bits: u8) -> Result<seL4_CPtr, seL4_Error> {
        if radix_bits == 0 || seL4_Word::from(radix_bits) >= word_bits() {
            return Err(seL4_RangeError);
        }
        let object_bits = (sel4_sys::seL4_SlotBits as u8)
            .checked_add(radix_bits)
            .ok_or(seL4_RangeError)?;
        let reserved = self
            .untyped
            .reserve_ram(object_bits)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_CapTableObject as seL4_Word,
            radix_bits as seL4_Word,
            slot,
        ) {
            Ok(()) => Ok(slot),
            Err(err) => {
                self.untyped.release(&reserved);
                Err(err.into_sel4_error())
            }
        }
    }

    /// Retypes and returns an AArch64 VSpace root object in the init CSpace.
    pub fn alloc_vspace_root(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(sel4_sys::seL4_VSpaceBits as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            slot,
            sel4_sys::seL4_ARM_VSpaceObject as seL4_Word,
            sel4_sys::seL4_VSpaceBits as seL4_Word,
            RetypeKind::VSpaceRoot,
        );
        self.record_retype(trace, RetypeStatus::Pending);
        match cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_ARM_VSpaceObject as seL4_Word,
            sel4_sys::seL4_VSpaceBits as seL4_Word,
            slot,
        ) {
            Ok(()) => {
                self.record_retype(trace, RetypeStatus::Ok);
                Ok(slot)
            }
            Err(err) => {
                let sel4_err = err.into_sel4_error();
                self.record_retype(trace, RetypeStatus::Err(sel4_err));
                self.untyped.release(&reserved);
                Err(sel4_err)
            }
        }
    }

    /// Assigns an ASID from the boot-provided root ASID pool to a VSpace cap.
    pub fn assign_vspace_asid_from_init_pool(&self, vspace: seL4_CPtr) -> Result<(), seL4_Error> {
        assign_vspace_asid(sel4_sys::seL4_CapInitThreadASIDPool, vspace)
    }

    /// Copies a capability into a fresh init-CNode slot with the supplied rights.
    pub fn copy_cap_to_new_slot(
        &mut self,
        source: seL4_CPtr,
        rights: sel4_sys::seL4_CapRights,
    ) -> Result<seL4_CPtr, seL4_Error> {
        let slot = self.allocate_slot();
        let depth = word_bits() as u8;
        let err = cnode_copy_depth(
            self.init_cnode_cap(),
            slot,
            depth,
            self.init_cnode_cap(),
            source,
            depth,
            rights,
        );
        if err == seL4_NoError {
            Ok(slot)
        } else {
            Err(err)
        }
    }

    /// Retype one child VSpace root (PGD) from a retained revoke anchor.
    ///
    /// `destination_slot` must be an empty init-CNode slot already owned by
    /// the caller. A failed ASID assignment leaves the object below `anchor`,
    /// so the caller can roll the partial generation back with
    /// [`Self::revoke_anchor_descendants_and_reset_vspace`].
    pub fn create_revoke_anchor_vspace_root(
        &self,
        anchor: seL4_CPtr,
        destination_slot: seL4_CPtr,
    ) -> Result<seL4_CPtr, RevokeAnchorVSpaceError> {
        if anchor == seL4_CapNull || destination_slot == seL4_CapNull || anchor == destination_slot
        {
            return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
        }
        self.retype_from_revoke_anchor(
            anchor,
            sel4_sys::seL4_ARM_VSpaceObject as seL4_Word,
            sel4_sys::seL4_VSpaceBits as seL4_Word,
            destination_slot,
        )?;
        self.assign_vspace_asid_from_init_pool(destination_slot)?;
        Ok(destination_slot)
    }

    /// Map one anchor-derived frame through anchor-derived PUD/PD/PT objects.
    ///
    /// The frame and VSpace root must have been retyped from `anchor`. Every
    /// missing translation object consumes the next fixed caller-owned slot in
    /// `tracker`; exhaustion is reported exactly and never falls back to the
    /// general untyped or CSpace allocator.
    pub fn map_page_cap_into_revoke_anchor_vspace<const N: usize>(
        &self,
        anchor: seL4_CPtr,
        frame: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<(), RevokeAnchorVSpaceError> {
        if anchor == seL4_CapNull || frame == seL4_CapNull || vspace == seL4_CapNull {
            return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
        }
        self.ensure_revoke_anchor_page_table(anchor, vspace, vaddr, tracker)?;
        map_page_into_vspace(frame, vspace, vaddr, rights, attr)?;
        Ok(())
    }

    /// Retype every unused tracker slot into an unmapped translation object.
    ///
    /// Exact resource inventories may reserve a fixed upper bound larger than
    /// the set of tables needed by the initial mappings. Call this only after
    /// all mappings for the generation are complete: the tracker is exhausted
    /// deliberately and further mapping attempts fail at the exact bound.
    pub fn seal_revoke_anchor_translation_reserve<const N: usize>(
        &self,
        anchor: seL4_CPtr,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<(), RevokeAnchorVSpaceError> {
        if anchor == seL4_CapNull {
            return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
        }
        while tracker.remaining_slots() != 0 {
            let slot = tracker.take_destination_slot()?;
            self.retype_from_revoke_anchor(
                anchor,
                seL4_ARM_PageTableObject as seL4_Word,
                PAGE_TABLE_BITS as seL4_Word,
                slot,
            )?;
        }
        Ok(())
    }

    fn ensure_revoke_anchor_page_table<const N: usize>(
        &self,
        anchor: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<(), RevokeAnchorVSpaceError> {
        self.ensure_revoke_anchor_page_directory(anchor, vspace, vaddr, tracker)?;
        let base = PageTableBookkeeper::<MAX_DRIVER_VSPACE_PAGE_TABLES>::base_for(vaddr);
        if tracker.tables.page_tables.contains_base(base) {
            return Ok(());
        }
        let slot = tracker.take_destination_slot()?;
        self.retype_from_revoke_anchor(
            anchor,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            slot,
        )?;
        map_page_table_into_vspace(slot, vspace, base, seL4_ARM_Page_Default)?;
        tracker
            .tables
            .page_tables
            .remember_base(base)
            .map_err(|_| RevokeAnchorVSpaceError::TranslationObjectBound)
    }

    fn ensure_revoke_anchor_page_directory<const N: usize>(
        &self,
        anchor: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<(), RevokeAnchorVSpaceError> {
        self.ensure_revoke_anchor_page_upper_directory(anchor, vspace, vaddr, tracker)?;
        let base = PageDirectoryBookkeeper::<MAX_DRIVER_VSPACE_PAGE_DIRECTORIES>::base_for(vaddr);
        if tracker.tables.page_directories.contains_base(base) {
            return Ok(());
        }
        let slot = tracker.take_destination_slot()?;
        self.retype_from_revoke_anchor(
            anchor,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            slot,
        )?;
        map_page_table_into_vspace(slot, vspace, base, seL4_ARM_Page_Default)?;
        tracker
            .tables
            .page_directories
            .remember_base(base)
            .map_err(|_| RevokeAnchorVSpaceError::TranslationObjectBound)
    }

    fn ensure_revoke_anchor_page_upper_directory<const N: usize>(
        &self,
        anchor: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<(), RevokeAnchorVSpaceError> {
        let base =
            PageUpperDirectoryBookkeeper::<MAX_DRIVER_VSPACE_PAGE_UPPER_DIRECTORIES>::base_for(
                vaddr,
            );
        if tracker.tables.page_upper_directories.contains_base(base) {
            return Ok(());
        }
        let slot = tracker.take_destination_slot()?;
        self.retype_from_revoke_anchor(
            anchor,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            slot,
        )?;
        map_page_table_into_vspace(slot, vspace, base, seL4_ARM_Page_Default)?;
        tracker
            .tables
            .page_upper_directories
            .remember_base(base)
            .map_err(|_| RevokeAnchorVSpaceError::TranslationObjectBound)
    }

    /// Revoke every anchor descendant and only then reset translation slots.
    ///
    /// The caller must suspend the child, unbind its SC, and close admission
    /// first. A failed kernel revoke preserves tracker state and therefore
    /// prevents unsafe destination-slot reuse.
    pub fn revoke_anchor_descendants_and_reset_vspace<const N: usize>(
        &mut self,
        anchor: seL4_CPtr,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<(), RevokeAnchorVSpaceError> {
        if anchor == seL4_CapNull {
            return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
        }
        let error = cnode_revoke(self.init_cnode_cap(), anchor, word_bits() as u8);
        if error != seL4_NoError {
            return Err(RevokeAnchorVSpaceError::Sel4(error));
        }
        tracker.reset_after_revoke();
        Ok(())
    }

    /// Maps a copied page capability into a non-root VSpace.
    pub fn map_page_copy_into_vspace(
        &mut self,
        source_frame: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<seL4_CPtr, seL4_Error> {
        self.ensure_page_table_in_vspace(vspace, vaddr, tracker)?;
        let frame_copy = self.copy_cap_to_new_slot(source_frame, rights)?;
        map_page_into_vspace(frame_copy, vspace, vaddr, rights, attr)?;
        Ok(frame_copy)
    }

    /// Maps an existing page capability into a non-root VSpace.
    pub fn map_page_cap_into_vspace(
        &mut self,
        frame: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<(), seL4_Error> {
        self.ensure_page_table_in_vspace(vspace, vaddr, tracker)?;
        map_page_into_vspace(frame, vspace, vaddr, rights, attr)
    }

    /// Removes the root VSpace mapping attached to a frame capability.
    pub fn unmap_page_cap(&mut self, frame: seL4_CPtr) -> Result<(), seL4_Error> {
        // SAFETY: `frame` is a frame capability allocated by this bootstrap
        // environment. seL4 validates the object type and returns a typed error
        // if the cap is not currently mapped.
        let result = unsafe { sel4_sys::seL4_ARM_Page_Unmap(frame) };
        if result == seL4_NoError {
            Ok(())
        } else {
            Err(result)
        }
    }

    /// Maps a physical device page into a non-root VSpace through the HAL cache.
    pub fn map_device_page_into_vspace(
        &mut self,
        paddr: usize,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<seL4_CPtr, seL4_Error> {
        if let Some(cached) = self.cached_device_frame_for_paddr(paddr) {
            if cached.exclusive_child_admission {
                return Err(sel4_sys::seL4_IllegalOperation);
            }
            return self.map_page_copy_into_vspace(
                cached.source_cap,
                vspace,
                vaddr,
                rights,
                attr,
                tracker,
            );
        }
        let frame_slot = self.retype_device_page_for_paddr(paddr, "driver-vspace-device")?;
        self.map_page_cap_into_vspace(frame_slot, vspace, vaddr, rights, attr, tracker)?;
        self.remember_device_frame_cap(paddr, frame_slot, false)?;
        Ok(frame_slot)
    }

    /// Consumes one pre-admitted, root-unmapped device capability into a child VSpace.
    ///
    /// On success the admission cache entry is removed, so later root
    /// `map_device` calls cannot discover the source capability and create a
    /// competing alias. The mapping capability remains live in the init CNode
    /// solely as the seL4 object backing the child mapping.
    pub fn map_admitted_device_page_exclusively_into_vspace(
        &mut self,
        paddr: usize,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<seL4_CPtr, seL4_Error> {
        let Some(index) = self.device_frame_cache.iter().position(|entry| {
            entry.paddr == paddr
                && entry.exclusive_child_admission
                && entry.root_cap.is_none()
                && entry.root_vaddr.is_none()
        }) else {
            return Err(sel4_sys::seL4_IllegalOperation);
        };
        let source_cap = self.device_frame_cache[index].source_cap;
        self.map_page_cap_into_vspace(source_cap, vspace, vaddr, rights, attr, tracker)?;
        let _ = self.device_frame_cache.remove(index);
        Ok(source_cap)
    }

    /// Maps one previously unclaimed device page exclusively into a child VSpace.
    ///
    /// Unlike [`Self::map_device_page_into_vspace`], this path deliberately does
    /// not retain a cache entry or create a root-VSpace alias. It is intended for
    /// large, child-owned device-memory ranges such as the boot framebuffer,
    /// where one permanent root alias and one copied child mapping per page would
    /// waste CSpace and violate the linked runtime's exclusive data-plane
    /// ownership. The returned init-CNode capability remains the mapping
    /// capability for the child VSpace and therefore must stay live.
    pub fn map_exclusive_device_page_into_vspace(
        &mut self,
        paddr: usize,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<seL4_CPtr, seL4_Error> {
        if self.cached_device_frame_for_paddr(paddr).is_some() {
            return Err(sel4_sys::seL4_IllegalOperation);
        }
        let frame_slot =
            self.retype_device_page_for_paddr(paddr, "driver-vspace-exclusive-device")?;
        self.map_page_cap_into_vspace(frame_slot, vspace, vaddr, rights, attr, tracker)?;
        Ok(frame_slot)
    }

    /// Retype one unclaimed device page into an exact caller-owned slot and
    /// map it through a revoke-anchor-owned child VSpace hierarchy.
    ///
    /// The device frame itself is not an anchor descendant, so the caller must
    /// suspend the child, reset/unmap the device, and delete `destination_slot`
    /// before revoking the generation anchor. Translation objects and every
    /// RAM DMA page remain anchor descendants.
    #[allow(clippy::too_many_arguments)]
    pub fn map_exclusive_device_page_into_revoke_anchor_vspace<const N: usize>(
        &mut self,
        anchor: seL4_CPtr,
        paddr: usize,
        destination_slot: seL4_CPtr,
        vspace: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        tracker: &mut RevokeAnchorVSpaceTracker<N>,
    ) -> Result<seL4_CPtr, RevokeAnchorVSpaceError> {
        if destination_slot == seL4_CapNull || self.cached_device_frame_for_paddr(paddr).is_some() {
            return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
        }
        let frame_slot = self
            .retype_device_page_for_paddr_into(
                paddr,
                "revoke-anchor-vspace-exclusive-device",
                Some(destination_slot),
            )
            .map_err(RevokeAnchorVSpaceError::Sel4)?;
        if let Err(error) = self.map_page_cap_into_revoke_anchor_vspace(
            anchor, frame_slot, vspace, vaddr, rights, attr, tracker,
        ) {
            let _ = cnode_delete(self.init_cnode_cap(), frame_slot, word_bits() as u8);
            return Err(error);
        }
        Ok(frame_slot)
    }

    /// Ensures that all intermediate page-table objects exist in a target VSpace.
    pub fn ensure_page_table_in_vspace(
        &mut self,
        vspace: seL4_CPtr,
        vaddr: usize,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<(), seL4_Error> {
        self.ensure_page_directory_in_vspace(vspace, vaddr, tracker)?;
        let pt_base = PageTableBookkeeper::<MAX_DRIVER_VSPACE_PAGE_TABLES>::base_for(vaddr);
        if tracker.page_tables.contains_base(pt_base) {
            return Ok(());
        }
        let pt_slot = self.allocate_translation_table_for_vaddr(
            pt_base,
            vaddr,
            RetypeKind::PageTable { vaddr: pt_base },
        )?;
        match map_page_table_into_vspace(pt_slot, vspace, pt_base, seL4_ARM_Page_Default) {
            Ok(()) => {
                tracker
                    .page_tables
                    .remember_base(pt_base)
                    .map_err(|_| seL4_NotEnoughMemory)?;
                Ok(())
            }
            Err(err) if Self::mapping_already_present(err) => {
                tracker
                    .page_tables
                    .remember_base(pt_base)
                    .map_err(|_| seL4_NotEnoughMemory)?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn ensure_page_directory_in_vspace(
        &mut self,
        vspace: seL4_CPtr,
        vaddr: usize,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<(), seL4_Error> {
        let pd_base =
            PageDirectoryBookkeeper::<MAX_DRIVER_VSPACE_PAGE_DIRECTORIES>::base_for(vaddr);
        if tracker.page_directories.contains_base(pd_base) {
            return Ok(());
        }
        self.ensure_page_upper_directory_in_vspace(vspace, vaddr, tracker)?;
        let pd_slot = self.allocate_translation_table_for_vaddr(
            pd_base,
            vaddr,
            RetypeKind::PageDirectory { vaddr: pd_base },
        )?;
        match map_page_table_into_vspace(pd_slot, vspace, pd_base, seL4_ARM_Page_Default) {
            Ok(()) => {
                tracker
                    .page_directories
                    .remember_base(pd_base)
                    .map_err(|_| seL4_NotEnoughMemory)?;
                Ok(())
            }
            Err(err) if Self::mapping_already_present(err) => {
                tracker
                    .page_directories
                    .remember_base(pd_base)
                    .map_err(|_| seL4_NotEnoughMemory)?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn ensure_page_upper_directory_in_vspace(
        &mut self,
        vspace: seL4_CPtr,
        vaddr: usize,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<(), seL4_Error> {
        let pud_base =
            PageUpperDirectoryBookkeeper::<MAX_DRIVER_VSPACE_PAGE_UPPER_DIRECTORIES>::base_for(
                vaddr,
            );
        if tracker.page_upper_directories.contains_base(pud_base) {
            return Ok(());
        }
        let pud_slot = self.allocate_translation_table_for_vaddr(
            pud_base,
            vaddr,
            RetypeKind::PageUpperDirectory { vaddr: pud_base },
        )?;
        match map_page_table_into_vspace(pud_slot, vspace, pud_base, seL4_ARM_Page_Default) {
            Ok(()) => {
                tracker
                    .page_upper_directories
                    .remember_base(pud_base)
                    .map_err(|_| seL4_NotEnoughMemory)?;
                Ok(())
            }
            Err(err) if Self::mapping_already_present(err) => {
                tracker
                    .page_upper_directories
                    .remember_base(pud_base)
                    .map_err(|_| seL4_NotEnoughMemory)?;
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn allocate_translation_table_for_vaddr(
        &mut self,
        table_base: usize,
        mapping_vaddr: usize,
        kind: RetypeKind,
    ) -> Result<seL4_CPtr, seL4_Error> {
        let level = match kind {
            RetypeKind::PageTable { .. } => "driver_page_table",
            RetypeKind::PageDirectory { .. } => "driver_page_directory",
            RetypeKind::PageUpperDirectory { .. } => "driver_page_upper_directory",
            _ => "driver_page_table",
        };
        let reserved = self.reserve_page_table_for_vaddr(table_base, mapping_vaddr, level)?;
        let slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            slot,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            kind,
        );
        self.record_retype(trace, RetypeStatus::Pending);
        match self.retype_page_table(reserved.cap(), &trace) {
            Ok(()) => {
                self.record_retype(trace, RetypeStatus::Ok);
                Ok(slot)
            }
            Err(err) => {
                self.record_retype(trace, RetypeStatus::Err(err));
                self.release_reserved_page_table(&reserved);
                Err(err)
            }
        }
    }

    /// Maps a physical device frame into the root task's device window.
    #[track_caller]
    pub fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, seL4_Error> {
        let caller = Location::caller();
        log::debug!(
            "[sel4.map_device] paddr=0x{paddr:08x} caller={}:{} device_cursor=0x{device_cursor:08x}",
            caller.file(),
            caller.line(),
            device_cursor = self.device_cursor,
        );
        if let Some(frame) = self.map_cached_device_frame(paddr)? {
            return Ok(frame);
        }
        let frame_slot = self.retype_device_page_for_paddr(paddr, "root-device")?;
        let range = self.next_mapping_range(self.device_cursor, PAGE_SIZE, "device-frame");
        self.device_cursor = range.end;
        self.map_frame(frame_slot, range.start, DEVICE_VM_ATTRIBUTES, false)?;
        if root_device_frame_cache_eligible(paddr) {
            self.record_cached_device_root_mapping(paddr, frame_slot, frame_slot, range.start)?;
        }
        Ok(DeviceFrame {
            cap: frame_slot,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(range.start))
                .expect("device mapping address must be non-null"),
        })
    }

    fn cached_device_frame_for_paddr(&self, paddr: usize) -> Option<DeviceFrameCacheEntry> {
        self.device_frame_cache
            .iter()
            .copied()
            .find(|entry| entry.paddr == paddr)
    }

    fn remember_device_frame_cap(
        &mut self,
        paddr: usize,
        source_cap: seL4_CPtr,
        exclusive_child_admission: bool,
    ) -> Result<(), seL4_Error> {
        if let Some(entry) = self
            .device_frame_cache
            .iter()
            .find(|entry| entry.paddr == paddr)
        {
            return if entry.exclusive_child_admission == exclusive_child_admission {
                Ok(())
            } else {
                Err(sel4_sys::seL4_IllegalOperation)
            };
        }
        self.device_frame_cache
            .push(DeviceFrameCacheEntry {
                paddr,
                source_cap,
                root_cap: None,
                root_vaddr: None,
                exclusive_child_admission,
            })
            .map_err(|_| seL4_NotEnoughMemory)
    }

    fn record_cached_device_root_mapping(
        &mut self,
        paddr: usize,
        source_cap: seL4_CPtr,
        root_cap: seL4_CPtr,
        root_vaddr: usize,
    ) -> Result<(), seL4_Error> {
        if let Some(entry) = self
            .device_frame_cache
            .iter_mut()
            .find(|entry| entry.paddr == paddr)
        {
            if entry.exclusive_child_admission {
                return Err(sel4_sys::seL4_IllegalOperation);
            }
            entry.root_cap = Some(root_cap);
            entry.root_vaddr = Some(root_vaddr);
            return Ok(());
        }
        self.device_frame_cache
            .push(DeviceFrameCacheEntry {
                paddr,
                source_cap,
                root_cap: Some(root_cap),
                root_vaddr: Some(root_vaddr),
                exclusive_child_admission: false,
            })
            .map_err(|_| seL4_NotEnoughMemory)
    }

    fn map_cached_device_frame(&mut self, paddr: usize) -> Result<Option<DeviceFrame>, seL4_Error> {
        let Some(cached) = self.cached_device_frame_for_paddr(paddr) else {
            return Ok(None);
        };
        if cached.exclusive_child_admission {
            return Err(sel4_sys::seL4_IllegalOperation);
        }
        if let (Some(root_cap), Some(root_vaddr)) = (cached.root_cap, cached.root_vaddr) {
            return Ok(Some(DeviceFrame {
                cap: root_cap,
                paddr,
                ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(root_vaddr))
                    .expect("device mapping address must be non-null"),
            }));
        }
        let root_cap = self.copy_cap_to_new_slot(cached.source_cap, seL4_CapRights_ReadWrite)?;
        let range = self.next_mapping_range(self.device_cursor, PAGE_SIZE, "device-frame-cache");
        self.device_cursor = range.end;
        self.map_frame(root_cap, range.start, DEVICE_VM_ATTRIBUTES, false)?;
        self.record_cached_device_root_mapping(paddr, cached.source_cap, root_cap, range.start)?;
        Ok(Some(DeviceFrame {
            cap: root_cap,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(range.start))
                .expect("device mapping address must be non-null"),
        }))
    }

    fn retype_device_page_for_paddr(
        &mut self,
        paddr: usize,
        label: &'static str,
    ) -> Result<seL4_CPtr, seL4_Error> {
        self.retype_device_page_for_paddr_into(paddr, label, None)
    }

    fn retype_device_page_for_paddr_into(
        &mut self,
        paddr: usize,
        label: &'static str,
        destination_slot: Option<seL4_CPtr>,
    ) -> Result<seL4_CPtr, seL4_Error> {
        let coverage = self
            .untyped
            .device_coverage(paddr, PAGE_BITS)
            .ok_or(seL4_NotEnoughMemory)?;
        let target_offset = paddr.saturating_sub(coverage.base);
        let mut current_offset = self
            .untyped
            .aligned_start_for_index(coverage.index, PAGE_BITS as u8)
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or(seL4_NotEnoughMemory)?;
        if current_offset > target_offset {
            log::warn!(
                "[sel4.retype_device] cannot map label={label} paddr=0x{paddr:08x}; untyped cursor advanced to 0x{cursor:08x}",
                cursor = coverage.base.saturating_add(current_offset),
            );
            return Err(seL4_NotEnoughMemory);
        }

        while current_offset < target_offset {
            let remaining = target_offset.saturating_sub(current_offset);
            let max_entry_bits = core::cmp::min(
                coverage.size_bits as usize,
                usize::BITS.saturating_sub(1) as usize,
            );
            let mut chunk_bits = core::cmp::min(
                max_entry_bits,
                (usize::BITS - 1 - remaining.leading_zeros()) as usize,
            );
            if chunk_bits < PAGE_BITS {
                chunk_bits = PAGE_BITS;
            }
            while chunk_bits > PAGE_BITS {
                let start = self
                    .untyped
                    .aligned_start_for_index(coverage.index, chunk_bits as u8)
                    .and_then(|offset| usize::try_from(offset).ok())
                    .ok_or(seL4_NotEnoughMemory)?;
                let size = 1usize << chunk_bits;
                if start.saturating_add(size) <= target_offset {
                    break;
                }
                chunk_bits -= 1;
            }
            let reserved_skip = self
                .untyped
                .reserve_device_from_index(coverage.index, chunk_bits as u8)
                .ok_or(seL4_NotEnoughMemory)?;
            self.retype_device_skip_object(&reserved_skip, chunk_bits as u8)?;
            current_offset = self
                .untyped
                .aligned_start_for_index(coverage.index, PAGE_BITS as u8)
                .and_then(|offset| usize::try_from(offset).ok())
                .ok_or(seL4_NotEnoughMemory)?;
        }

        let reserved = self
            .untyped
            .reserve_device_from_index(coverage.index, PAGE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        if reserved.paddr() != paddr {
            log::warn!(
                "[sel4.retype_device] reservation mismatch label={label} target=0x{target:08x} reserved=0x{reserved:08x} ut=0x{cap:03x}",
                target = paddr,
                reserved = reserved.paddr(),
                cap = reserved.cap(),
            );
            self.untyped.release(&reserved);
            return Err(seL4_NotEnoughMemory);
        }
        let frame_slot = match destination_slot {
            Some(slot) if slot != seL4_CapNull => slot,
            Some(_) => {
                self.untyped.release(&reserved);
                return Err(sel4_sys::seL4_InvalidCapability);
            }
            None => match self.try_allocate_slot() {
                Ok(slot) => slot,
                Err(error) => {
                    self.untyped.release(&reserved);
                    return Err(error);
                }
            },
        };
        #[cfg(target_arch = "aarch64")]
        let page_obj: seL4_Word = SEL4_ARM_PAGE_OBJECT_WORD;
        #[cfg(target_arch = "aarch64")]
        let page_bits: seL4_Word = 12;

        #[cfg(not(target_arch = "aarch64"))]
        compile_error!("Wire correct page object type/size for non-AArch64 targets.");

        let dev_span = 1usize
            .checked_shl(reserved.size_bits() as u32)
            .expect("device untyped size_bits must fit host word size");
        log::trace!(
            "device_untyped chosen: label={} cap=0x{:x} idx={} covers=[0x{:08x}..0x{:08x}) size_bits={} target=0x{:08x}",
            label,
            reserved.cap(),
            reserved.index(),
            reserved.paddr() as u64,
            reserved.paddr().saturating_add(dev_span) as u64,
            reserved.size_bits(),
            paddr as u64
        );

        let trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            page_obj,
            page_bits,
            RetypeKind::DevicePage { paddr },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        let actual_paddr = match page_get_address(frame_slot) {
            Ok(addr) => addr,
            Err(err) => {
                self.record_retype(trace, RetypeStatus::Err(err));
                return Err(err);
            }
        };
        if actual_paddr != paddr {
            log::warn!(
                "[sel4.retype_device] cap paddr mismatch label={label} target=0x{target:08x} actual=0x{actual:08x} slot=0x{slot:04x}",
                target = paddr,
                actual = actual_paddr,
                slot = frame_slot,
            );
            self.record_retype(trace, RetypeStatus::Err(seL4_RangeError));
            return Err(seL4_RangeError);
        }
        self.record_retype(trace, RetypeStatus::Ok);
        Ok(frame_slot)
    }

    fn retype_device_skip_object(
        &mut self,
        reserved: &ReservedUntyped,
        object_bits: u8,
    ) -> Result<(), seL4_Error> {
        let slot = self.allocate_slot();
        let result = cspace_sys::untyped_retype_into_init_root(
            reserved.cap() as seL4_CPtr,
            sel4_sys::seL4_UntypedObject as seL4_Word,
            object_bits as seL4_Word,
            slot,
        );
        match result {
            Ok(()) => self
                .device_skip_objects
                .push(slot)
                .map_err(|_| seL4_NotEnoughMemory),
            Err(err) => {
                self.untyped.release(reserved);
                Err(err.into_sel4_error())
            }
        }
    }

    /// Maps the init thread's IPC buffer frame into the supplied virtual address.
    pub fn map_ipc_buffer(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        assert_ne!(vaddr, 0, "IPC buffer pointer must be non-null");
        assert_eq!(
            vaddr & ((1 << IPC_PAGE_BITS) - 1),
            0,
            "IPC buffer pointer must be aligned to the page size",
        );

        let (l1, l2, l3, page) = Self::translation_indices(vaddr);
        let pt_base = PageTableBookkeeper::<MAX_PAGE_TABLES>::base_for(vaddr);
        let bootinfo_addr = self.bootinfo as *const _ as usize;
        let bootinfo_base = PageTableBookkeeper::<MAX_PAGE_TABLES>::base_for(bootinfo_addr);

        ::log::info!(
            "[boot] ipcbuf translation indices l1={l1:#05x} l2={l2:#05x} l3={l3:#05x} page={page:#05x} base=0x{pt_base:08x} page_bits={page_bits}",
            page_bits = sel4_sys::seL4_PageBits,
        );

        if pt_base != bootinfo_base {
            ::log::warn!(
                "[boot] ipcbuf L3 base 0x{pt_base:08x} diverges from bootinfo base 0x{bootinfo_base:08x}; proceeding with page-table allocation",
            );
        }

        self.ipcbuf_trace = true;
        let res = self.map_frame(
            seL4_CapInitThreadIPCBuffer,
            vaddr,
            seL4_ARM_Page_Default,
            false,
        );
        self.ipcbuf_trace = false;

        if res.is_ok() {
            self.guard_bootinfo_access();
        }

        res
    }

    pub(crate) fn log_ipc_buffer_cap(
        &self,
        buffer_frame: seL4_CPtr,
        buffer_vaddr: usize,
    ) -> Option<CapTag> {
        #[cfg(target_os = "none")]
        let cap_tag_raw = debug_cap_identify(buffer_frame);
        #[cfg(not(target_os = "none"))]
        let cap_tag_raw = CapTag::Frame as seL4_Word;

        #[cfg(target_os = "none")]
        if !debug_cap_identify_available() {
            ::log::info!(
                "[ipcbuf] capid unavailable frame=0x{buffer_frame:04x} vaddr=0x{buffer_vaddr:08x}"
            );
            return None;
        }

        let cap_tag = CapTag::from_raw(cap_tag_raw as seL4_Word);

        ::log::info!(
            "[ipcbuf] capid frame=0x{buffer_frame:04x} ty=0x{cap_tag_raw:08x} ({tag}) vaddr=0x{buffer_vaddr:08x}",
            buffer_frame = buffer_frame,
            cap_tag_raw = cap_tag_raw,
            tag = cap_tag.map(CapTag::name).unwrap_or("unknown"),
            buffer_vaddr = buffer_vaddr,
        );

        if !matches!(cap_tag, Some(CapTag::Frame)) {
            ::log::warn!(
                "[ipcbuf] unexpected cap type for IPC buffer: 0x{cap_tag_raw:08x} ({tag})",
                tag = cap_tag.map(CapTag::name).unwrap_or("unknown"),
            );
        }

        cap_tag
    }

    /// Binds the supplied IPC buffer frame to the provided TCB capability.
    ///
    /// This configures a non-current TCB and deliberately does not replace the
    /// root task's active IPC buffer pointer.
    pub fn bind_remote_ipc_buffer(
        &mut self,
        tcb_cap: seL4_CPtr,
        buffer_frame: seL4_CPtr,
        buffer_vaddr: usize,
    ) -> Result<(), seL4_Error> {
        debug_assert_ne!(buffer_vaddr, 0, "IPC buffer pointer must be non-null");
        let _cap_tag = self.log_ipc_buffer_cap(buffer_frame, buffer_vaddr);
        let buffer_word = sel4_sys::seL4_Word::try_from(buffer_vaddr)
            .expect("IPC buffer pointer must fit in seL4_Word");
        let guard_stage = "IPCInstall.bind_remote_ipc_buffer";
        let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
        let guarded_frame = sel4_guard::guard_cptr(guard_stage, "ipc_frame", buffer_frame);
        let mut breadcrumb = HeaplessString::<192>::new();
        let _ = fmt::write(
            &mut breadcrumb,
            format_args!(
                "remote_tcb=0x{tcb:04x} buffer=0x{buffer:08x} frame=0x{frame:04x}",
                tcb = guarded_tcb,
                buffer = buffer_word,
                frame = guarded_frame
            ),
        );
        sel4_guard::uart_breadcrumb(guard_stage, "seL4_TCB_SetIPCBuffer", breadcrumb.as_str());
        // SAFETY: The guarded TCB and frame are kernel capabilities supplied by bootstrap code;
        // seL4 validates object types and IPC-buffer alignment.
        let result =
            unsafe { sel4_sys::seL4_TCB_SetIPCBuffer(guarded_tcb, buffer_word, guarded_frame) };
        if result == seL4_NoError {
            Ok(())
        } else {
            ::log::error!(
                "[ipcbuf] remote bind failed tcb=0x{tcb:04x} frame=0x{frame:04x} vaddr=0x{vaddr:08x} err={err} ({name})",
                tcb = guarded_tcb,
                frame = guarded_frame,
                vaddr = buffer_vaddr,
                err = result,
                name = error_name(result),
            );
            Err(result)
        }
    }

    /// Binds the supplied IPC buffer frame to the current root TCB capability.
    pub fn bind_ipc_buffer(
        &mut self,
        tcb_cap: seL4_CPtr,
        buffer_frame: seL4_CPtr,
        buffer_vaddr: usize,
    ) -> Result<IpcBufView, seL4_Error> {
        debug_assert_ne!(buffer_vaddr, 0, "IPC buffer pointer must be non-null");
        if self.ipcbuf_trace {
            crate::bp!("ipcbuf.tcb.bind.begin");
        }

        let _cap_tag = self.log_ipc_buffer_cap(buffer_frame, buffer_vaddr);
        let buffer_word = sel4_sys::seL4_Word::try_from(buffer_vaddr)
            .expect("IPC buffer pointer must fit in seL4_Word");

        ::log::info!(
            "[ffi] seL4_TCB_SetIPCBuffer service=0x{tcb_cap:04x} buffer=0x{buffer_word:08x} frame=0x{buffer_frame:04x}",
            tcb_cap = tcb_cap,
            buffer_word = buffer_word,
            buffer_frame = buffer_frame,
        );

        let guard_stage = "IPCInstall.bind_ipc_buffer";
        let guarded_tcb = sel4_guard::guard_cptr(guard_stage, "tcb_cap", tcb_cap);
        let guarded_frame = sel4_guard::guard_cptr(guard_stage, "ipc_frame", buffer_frame);
        let tcb_cap = guarded_tcb;
        let buffer_frame = guarded_frame;
        let mut breadcrumb = HeaplessString::<192>::new();
        let _ = fmt::write(
            &mut breadcrumb,
            format_args!(
                "tcb=0x{tcb:04x} buffer=0x{buffer:08x} frame=0x{frame:04x}",
                tcb = guarded_tcb,
                buffer = buffer_word,
                frame = guarded_frame
            ),
        );
        sel4_guard::uart_breadcrumb(guard_stage, "seL4_TCB_SetIPCBuffer", breadcrumb.as_str());
        // SAFETY: The guarded TCB and frame are kernel capabilities supplied by bootstrap code;
        // seL4 validates object types and IPC-buffer alignment.
        let result =
            unsafe { sel4_sys::seL4_TCB_SetIPCBuffer(guarded_tcb, buffer_word, guarded_frame) };

        if result == seL4_NoError {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.tcb.bind.ok");
            }
            #[cfg(target_os = "none")]
            // SAFETY: This method is only used for the current root TCB; after a successful
            // kernel bind, the root task must update the local libsel4 IPC buffer pointer.
            unsafe {
                sel4_sys::seL4_SetIPCBuffer(buffer_vaddr as *mut sel4_sys::seL4_IPCBuffer);
            }
            #[cfg(not(target_os = "none"))]
            sel4_sys::seL4_SetIPCBuffer(buffer_vaddr as *mut sel4_sys::seL4_IPCBuffer);
            // SAFETY: `buffer_vaddr` names the mapped IPC buffer frame installed above.
            let view = unsafe { IpcBufView::new(buffer_vaddr as *const u8, buffer_frame) };
            self.ipcbuf_view = Some(view);
            // SAFETY: The IPC buffer page is mapped and page-sized; touching first/last byte
            // validates the mapping without relying on compiler-elided ordinary loads/stores.
            unsafe {
                let base = buffer_vaddr as *mut u8;
                let last = base.add(IpcBufView::PAGE_LEN - 1);
                let first_value = core::ptr::read_volatile(base);
                core::ptr::write_volatile(base, first_value);
                let last_value = core::ptr::read_volatile(last);
                core::ptr::write_volatile(last, last_value);
            }
            Ok(view)
        } else {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.tcb.bind.err");
            }
            ::log::error!(
                "[ipcbuf] bind failed tcb=0x{tcb:04x} frame=0x{frame:04x} vaddr=0x{vaddr:08x} err={err} ({name})",
                tcb = tcb_cap,
                frame = buffer_frame,
                vaddr = buffer_vaddr,
                err = result,
                name = error_name(result),
            );
            Err(result)
        }
    }

    /// Installs an IPC buffer for a child TCB without changing root's libsel4 pointer.
    ///
    /// The frame remains mapped in root for bounded shared-record access, but
    /// `__sel4_ipc_buffer` belongs to the calling TCB and must never be switched
    /// while configuring a remote child.
    pub fn bind_child_ipc_buffer(
        &self,
        tcb_cap: seL4_CPtr,
        buffer_frame: seL4_CPtr,
        buffer_vaddr: usize,
    ) -> Result<IpcBufView, seL4_Error> {
        if buffer_vaddr == 0 || buffer_vaddr & (IpcBufView::PAGE_LEN - 1) != 0 {
            return Err(sel4_sys::seL4_AlignmentError);
        }
        let buffer_word = seL4_Word::try_from(buffer_vaddr).map_err(|_| seL4_RangeError)?;
        let guarded_tcb = sel4_guard::guard_cptr("IPCInstall.bind_child", "tcb_cap", tcb_cap);
        let guarded_frame =
            sel4_guard::guard_cptr("IPCInstall.bind_child", "ipc_frame", buffer_frame);
        // SAFETY: The guarded TCB and frame are bootstrap-owned kernel
        // capabilities. The frame is page-aligned and mapped at `buffer_word`;
        // seL4 validates the object types and target TCB configuration.
        let result =
            unsafe { sel4_sys::seL4_TCB_SetIPCBuffer(guarded_tcb, buffer_word, guarded_frame) };
        if result != seL4_NoError {
            return Err(result);
        }
        // SAFETY: `buffer_vaddr` names the still-live page mapping whose frame
        // was installed above; the returned view does not mutate root's global
        // libsel4 IPC-buffer pointer.
        Ok(unsafe { IpcBufView::new(buffer_vaddr as *const u8, guarded_frame) })
    }

    /// Allocates a DMA-capable frame of RAM and maps it into the DMA window.
    pub fn alloc_dma_frame(&mut self) -> Result<RamFrame, seL4_Error> {
        self.alloc_dma_frame_attr(seL4_ARM_Page_Default)
    }

    /// Allocates a DMA-capable frame from the lowest-address RAM and maps it.
    pub fn alloc_dma_frame_low(&mut self) -> Result<RamFrame, seL4_Error> {
        self.alloc_dma_frame_low_attr(seL4_ARM_Page_Default)
    }

    /// Allocates a DMA-capable frame and maps it with the supplied cache attribute.
    pub fn alloc_dma_frame_attr(
        &mut self,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, seL4_Error> {
        self.alloc_dma_frame_attr_inner(attr, true)
    }

    /// Allocates a low-address DMA-capable frame with the supplied cache attribute.
    pub fn alloc_dma_frame_low_attr(
        &mut self,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, seL4_Error> {
        self.alloc_dma_frame_attr_inner(attr, false)
    }

    /// Allocates a DMA-capable RAM frame without mapping it into the root VSpace.
    ///
    /// This is for driver-local runtime buffers that are mapped only into a
    /// child driver VSpace. Root-visible shared buffers should continue to use
    /// [`KernelEnv::alloc_dma_frame_attr`] so the root task can act as the ring
    /// client without raw driver-state pointers.
    pub fn alloc_unmapped_ram_frame_attr(
        &mut self,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<UnmappedRamFrame, seL4_Error> {
        self.alloc_unmapped_ram_frame_attr_inner(attr, true)
    }

    /// Allocates a low-address DMA-capable RAM frame without a root mapping.
    ///
    /// This is reserved for child-owned devices such as the Pi firmware
    /// mailbox whose bus protocol carries only a 30-bit request-page address.
    pub fn alloc_unmapped_ram_frame_low_attr(
        &mut self,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<UnmappedRamFrame, seL4_Error> {
        self.alloc_unmapped_ram_frame_attr_inner(attr, false)
    }

    fn alloc_unmapped_ram_frame_attr_inner(
        &mut self,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        prefer_high: bool,
    ) -> Result<UnmappedRamFrame, seL4_Error> {
        let trace_uncached = attr == sel4_sys::seL4_ARM_Page_Uncached;
        let trace_verbose = trace_uncached && LOCAL_SEAT_DMA_FRAME_VERBOSE_LOGS;
        if trace_verbose {
            let mut line = HeaplessString::<160>::new();
            let _ = write!(
                &mut line,
                "[driver-runtime] unmapped-ram-frame begin source={} attr=0x{:08x}",
                if prefer_high { "high" } else { "low" },
                vm_attributes_raw(attr) as u32
            );
            boot_log::force_uart_line(line.as_str());
        }
        BOOTINFO_WINDOW_GUARD.check("alloc_unmapped_ram_frame_attr");
        let reserved = if prefer_high {
            self.untyped.reserve_ram_high(PAGE_BITS as u8)
        } else {
            self.untyped.reserve_ram(PAGE_BITS as u8)
        }
        .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        let mut trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            SEL4_ARM_PAGE_OBJECT_WORD,
            PAGE_BITS as seL4_Word,
            RetypeKind::DmaPage { paddr: 0 },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page(reserved.cap(), &trace) {
            if trace_uncached {
                let mut line = HeaplessString::<208>::new();
                let _ = write!(
                    &mut line,
                    "[driver-runtime] unmapped-ram-frame retype failed ut=0x{:04x} slot=0x{:04x} err={} ({})",
                    reserved.cap(),
                    frame_slot,
                    err,
                    error_name(err)
                );
                boot_log::force_uart_line(line.as_str());
            }
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        let paddr = match page_get_address(frame_slot) {
            Ok(paddr) => paddr,
            Err(err) => {
                if trace_uncached {
                    let mut line = HeaplessString::<192>::new();
                    let _ = write!(
                        &mut line,
                        "[driver-runtime] unmapped-ram-frame paddr failed slot=0x{:04x} err={} ({})",
                        frame_slot,
                        err,
                        error_name(err)
                    );
                    boot_log::force_uart_line(line.as_str());
                }
                self.record_retype(trace, RetypeStatus::Err(err));
                return Err(err);
            }
        };
        trace.kind = RetypeKind::DmaPage { paddr };
        self.record_retype(trace, RetypeStatus::Ok);
        ::log::debug!(
            target: "hal",
            "[hal] unmapped ram frame allocated source={} slot=0x{slot:04x} paddr=0x{paddr:08x} attr=0x{attr:08x}",
            if prefer_high { "high" } else { "low" },
            slot = frame_slot,
            paddr = paddr,
            attr = vm_attributes_raw(attr) as usize,
        );
        Ok(UnmappedRamFrame {
            cap: frame_slot,
            paddr,
        })
    }

    fn alloc_dma_frame_attr_inner(
        &mut self,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        prefer_high: bool,
    ) -> Result<RamFrame, seL4_Error> {
        let trace_uncached = attr == sel4_sys::seL4_ARM_Page_Uncached;
        let trace_verbose = trace_uncached && LOCAL_SEAT_DMA_FRAME_VERBOSE_LOGS;
        if trace_verbose {
            let mut line = HeaplessString::<160>::new();
            let _ = write!(
                &mut line,
                "[local-seat] dma-frame begin source={} attr=0x{:08x}",
                if prefer_high { "high" } else { "low" },
                vm_attributes_raw(attr) as u32
            );
            boot_log::force_uart_line(line.as_str());
        }
        BOOTINFO_WINDOW_GUARD.check(if prefer_high {
            "alloc_dma_frame_attr"
        } else {
            "alloc_dma_frame_low_attr"
        });
        let reserved = if prefer_high {
            self.untyped.reserve_ram_high(PAGE_BITS as u8)
        } else {
            self.untyped.reserve_ram(PAGE_BITS as u8)
        }
        .ok_or(seL4_NotEnoughMemory)?;
        if trace_verbose {
            let mut line = HeaplessString::<176>::new();
            let _ = write!(
                &mut line,
                "[local-seat] dma-frame reserved ut=0x{:04x}",
                reserved.cap()
            );
            boot_log::force_uart_line(line.as_str());
        }
        let frame_slot = match self.try_allocate_slot() {
            Ok(slot) => slot,
            Err(error) => {
                self.untyped.release(&reserved);
                return Err(error);
            }
        };
        let mut trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            SEL4_ARM_PAGE_OBJECT_WORD,
            PAGE_BITS as seL4_Word,
            RetypeKind::DmaPage { paddr: 0 },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page(reserved.cap(), &trace) {
            if trace_uncached {
                let mut line = HeaplessString::<192>::new();
                let _ = write!(
                    &mut line,
                    "[local-seat] dma-frame retype failed ut=0x{:04x} slot=0x{:04x} err={} ({})",
                    reserved.cap(),
                    frame_slot,
                    err,
                    error_name(err)
                );
                boot_log::force_uart_line(line.as_str());
            }
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        if trace_verbose {
            let mut line = HeaplessString::<176>::new();
            let _ = write!(
                &mut line,
                "[local-seat] dma-frame retype ok slot=0x{:04x}",
                frame_slot
            );
            boot_log::force_uart_line(line.as_str());
        }
        let paddr = match page_get_address(frame_slot) {
            Ok(paddr) => paddr,
            Err(err) => {
                if trace_uncached {
                    let mut line = HeaplessString::<192>::new();
                    let _ = write!(
                        &mut line,
                        "[local-seat] dma-frame paddr failed slot=0x{:04x} err={} ({})",
                        frame_slot,
                        err,
                        error_name(err)
                    );
                    boot_log::force_uart_line(line.as_str());
                }
                self.record_retype(trace, RetypeStatus::Err(err));
                return Err(err);
            }
        };
        if trace_verbose {
            let mut line = HeaplessString::<192>::new();
            let _ = write!(
                &mut line,
                "[local-seat] dma-frame paddr slot=0x{:04x} paddr=0x{:016x}",
                frame_slot, paddr
            );
            boot_log::force_uart_line(line.as_str());
        }
        trace.kind = RetypeKind::DmaPage { paddr };
        self.record_retype(trace, RetypeStatus::Ok);
        let range = self.next_mapping_range(self.dma_cursor, PAGE_SIZE, "dma-frame");
        self.dma_cursor = range.end;
        if trace_verbose {
            let mut line = HeaplessString::<208>::new();
            let _ = write!(
                &mut line,
                "[local-seat] dma-frame map begin slot=0x{:04x} vaddr=0x{:08x}",
                frame_slot, range.start
            );
            boot_log::force_uart_line(line.as_str());
        }
        self.map_frame(frame_slot, range.start, attr, true)?;
        if trace_verbose {
            let mut line = HeaplessString::<176>::new();
            let _ = write!(
                &mut line,
                "[local-seat] dma-frame map ok slot=0x{:04x}",
                frame_slot
            );
            boot_log::force_uart_line(line.as_str());
        }
        let attr_raw = vm_attributes_raw(attr) as usize;
        record_dma_mapping(paddr, range.start, PAGE_SIZE, attr_raw);
        if trace_verbose {
            boot_log::force_uart_line("[local-seat] dma-frame before hal-log");
        }
        ::log::debug!(
            target: "hal",
            "[hal] dma frame mapped source={source} vaddr=0x{vaddr:08x} paddr=0x{paddr:08x} attr=0x{attr:08x}",
            source = if prefer_high { "high" } else { "low" },
            vaddr = range.start,
            paddr = paddr,
            attr = attr_raw,
        );
        if trace_verbose {
            boot_log::force_uart_line("[local-seat] dma-frame after hal-log");
        }
        Ok(RamFrame {
            cap: frame_slot,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(range.start))
                .expect("DMA mapping address must be non-null"),
        })
    }

    /// Reserves an unmapped guard page in the DMA window and returns its base.
    pub fn reserve_dma_guard_page(&mut self) -> usize {
        let range = self.next_mapping_range(self.dma_cursor, PAGE_SIZE, "dma-guard");
        self.dma_cursor = range.end;
        self.reserved.reserve(&range, "dma-guard");
        range.start
    }

    fn retype_page(
        &mut self,
        untyped_cap: seL4_Untyped,
        trace: &RetypeTrace,
    ) -> Result<(), seL4_Error> {
        debug_assert!(
            matches!(
                trace.kind,
                RetypeKind::DevicePage { .. } | RetypeKind::DmaPage { .. }
            ),
            "retype_page expects a page-related trace"
        );
        debug_assert_eq!(
            trace.object_type, SEL4_ARM_PAGE_OBJECT_WORD,
            "ARM device/RAM frames must use seL4_ARM_Page",
        );
        debug_assert_eq!(
            trace.object_size_bits, PAGE_BITS as seL4_Word,
            "ARM device/RAM frames must have 4KiB size bits"
        );

        let (trace, _init_bits) = self.sanitise_retype_trace(*trace);
        self.log_retype_invocation(&trace);

        #[cfg(target_arch = "aarch64")]
        if matches!(trace.kind, RetypeKind::DevicePage { .. }) {
            debug_assert_eq!(
                trace.object_type, SEL4_ARM_PAGE_OBJECT_WORD,
                "Device page retype must use seL4_ARM_Page on AArch64"
            );
            debug_assert_eq!(
                trace.object_size_bits, 12,
                "AArch64 page size must be 12 bits (4 KiB)"
            );
        }

        let res = if trace.cnode_root == self.bootinfo.init_cnode_cap() {
            match cspace_sys::untyped_retype_into_init_root(
                untyped_cap as seL4_CPtr,
                trace.object_type,
                trace.object_size_bits,
                trace.dest_slot,
            ) {
                Ok(()) => seL4_NoError,
                Err(err) => err.into_sel4_error(),
            }
        } else {
            // SAFETY: The trace has been sanitised to a caller-owned CNode
            // destination tuple and requests exactly one 4 KiB ARM page object
            // from the supplied untyped capability; seL4 validates capability
            // authority and object availability.
            unsafe {
                seL4_Untyped_Retype(
                    untyped_cap,
                    trace.object_type,
                    trace.object_size_bits,
                    trace.cnode_root,
                    trace.node_index,
                    u64::from(trace.cnode_depth as u8),
                    trace.dest_offset,
                    1,
                )
            }
        };

        if res == seL4_NoError {
            Ok(())
        } else {
            Err(res)
        }
    }

    fn retype_page_table(
        &mut self,
        untyped_cap: seL4_Untyped,
        trace: &RetypeTrace,
    ) -> Result<(), seL4_Error> {
        debug_assert_eq!(
            trace.object_type, seL4_ARM_PageTableObject as seL4_Word,
            "Page table retype must target seL4_ARM_PageTableObject",
        );
        debug_assert_eq!(
            trace.object_size_bits, PAGE_TABLE_BITS as seL4_Word,
            "Page table retype must use seL4_PageTableBits",
        );
        let (trace, _init_bits) = self.sanitise_retype_trace(*trace);
        self.log_retype_invocation(&trace);

        let res = if trace.cnode_root == self.bootinfo.init_cnode_cap() {
            match cspace_sys::untyped_retype_into_init_root(
                untyped_cap as seL4_CPtr,
                trace.object_type,
                trace.object_size_bits,
                trace.dest_slot,
            ) {
                Ok(()) => seL4_NoError,
                Err(err) => err.into_sel4_error(),
            }
        } else {
            // SAFETY: The trace has been sanitised to a caller-owned CNode
            // destination tuple and requests exactly one ARM page-table object
            // from the supplied untyped capability; seL4 validates capability
            // authority and object availability.
            unsafe {
                seL4_Untyped_Retype(
                    untyped_cap,
                    trace.object_type,
                    trace.object_size_bits,
                    trace.cnode_root,
                    trace.node_index,
                    u64::from(trace.cnode_depth as u8),
                    trace.dest_offset,
                    1,
                )
            }
        };

        if res == seL4_NoError {
            Ok(())
        } else {
            Err(res)
        }
    }

    fn sanitise_retype_trace(&self, trace: RetypeTrace) -> (RetypeTrace, usize) {
        let init_bits = self.bootinfo.init_cnode_bits();
        let (empty_start, empty_end) = self.bootinfo.init_cnode_empty_usize();
        let slot_limit = 1usize.checked_shl(init_bits as u32).unwrap_or_else(|| {
            panic!(
                "initThreadCNodeSizeBits {} exceeds host word size",
                init_bits
            )
        });
        let init_cnode = self.bootinfo.init_cnode_cap();
        let expected_depth: seL4_Word = cspace_sys::canonical_depth_word();
        let expected_index: seL4_Word = cspace_sys::init_root_index();
        let expected_offset: seL4_Word = trace.dest_slot as seL4_Word;
        assert!(
            (trace.dest_slot as usize) < slot_limit,
            "Retype: dest_slot 0x{:x} out of range for init_bits={} (limit=0x{:x})",
            trace.dest_slot,
            init_bits,
            slot_limit,
        );
        assert!(
            (trace.dest_slot as usize) >= empty_start && (trace.dest_slot as usize) < empty_end,
            "Retype: dest_slot 0x{slot:04x} outside empty window [0x{start:04x}..0x{end:04x})",
            slot = trace.dest_slot,
            start = empty_start,
            end = empty_end,
        );

        let sanitised = trace;
        assert_eq!(
            trace.cnode_root, init_cnode,
            "Retype: cnode_root 0x{:04x} must match init cnode 0x{:04x}",
            trace.cnode_root, init_cnode,
        );
        assert_eq!(
            trace.cnode_depth, expected_depth,
            "Retype: cnode_depth {} must match canonical depth {}",
            trace.cnode_depth, expected_depth,
        );

        let node_index = sanitised.node_index;
        assert!(
            (node_index as usize) < slot_limit,
            "Retype: node_index 0x{node_index:04x} out of range for init_bits={init_bits} (limit=0x{slot_limit:x})",
        );
        assert_eq!(
            node_index, expected_index,
            "Retype: node_index 0x{:04x} must match init root index 0x{:04x}",
            node_index, expected_index,
        );

        let dest_offset = sanitised.dest_offset;
        assert!(
            (dest_offset as usize) < slot_limit,
            "Retype: dest_offset 0x{dest_offset:04x} out of range for init_bits={init_bits} (limit=0x{slot_limit:x})",
        );
        assert_eq!(
            dest_offset, expected_offset,
            "Retype: dest_offset 0x{:04x} must match dest_slot 0x{:04x}",
            dest_offset, expected_offset,
        );

        (sanitised, init_bits)
    }

    fn map_frame(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        strict: bool,
    ) -> Result<(), seL4_Error> {
        self.map_frame_with_rights(frame_cap, vaddr, seL4_CapRights_ReadWrite, attr, strict)
    }

    fn map_frame_with_rights(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        strict: bool,
    ) -> Result<(), seL4_Error> {
        Self::assert_page_aligned(vaddr);

        let end = vaddr
            .checked_add(PAGE_SIZE)
            .expect("virtual address calculation overflow");
        self.assert_reserved_clear(vaddr..end, "map_frame");

        let mut result = self.attempt_page_map_with_rights(frame_cap, vaddr, rights, attr);
        if result == seL4_NoError {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.page.map.ok");
            }
            return Ok(());
        }

        if !strict && Self::mapping_already_present(result) {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.page.map.ok");
            }
            return Ok(());
        }

        if result == sel4_sys::seL4_FailedLookup {
            self.ensure_page_table(vaddr)?;
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.page.map.retry");
            }
            result = self.attempt_page_map_with_rights(frame_cap, vaddr, rights, attr);
            if result == seL4_NoError {
                if self.ipcbuf_trace {
                    crate::bp!("ipcbuf.page.map.ok");
                }
                return Ok(());
            }

            if !strict && Self::mapping_already_present(result) {
                if self.ipcbuf_trace {
                    crate::bp!("ipcbuf.page.map.ok");
                }
                return Ok(());
            }
        }

        let _ = crate::bootstrap::ktry("ipcbuf.page.map", result as i32);
        Err(result)
    }

    fn align_down(value: usize, align: usize) -> usize {
        debug_assert!(align.is_power_of_two());
        value & !(align - 1)
    }

    fn assert_reserved_clear(&self, range: core::ops::Range<usize>, label: &str) {
        self.reserved.assert_free(&range, label);
    }

    fn next_mapping_range(
        &self,
        cursor: usize,
        span: usize,
        label: &str,
    ) -> core::ops::Range<usize> {
        let range = self.reserved.next_aligned_range(cursor, span, PAGE_SIZE);
        self.assert_reserved_clear(range.clone(), label);
        range
    }

    fn attempt_page_map(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        self.attempt_page_map_with_rights(frame_cap, vaddr, seL4_CapRights_ReadWrite, attr)
    }

    fn attempt_page_map_with_rights(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        rights: sel4_sys::seL4_CapRights,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> seL4_Error {
        if self.ipcbuf_trace {
            crate::bp!("ipcbuf.page.map.begin");
        }
        let vaddr_word =
            sel4_sys::seL4_Word::try_from(vaddr).expect("virtual address must fit in seL4_Word");
        // SAFETY: `frame_cap` names an ARM page capability allocated by this
        // HAL, `vaddr` was checked page-aligned, and the init VSpace cap is the
        // kernel-provided root-task VSpace. The kernel validates page-table
        // presence and mapping authority.
        unsafe {
            seL4_ARM_Page_Map(
                frame_cap,
                seL4_CapInitThreadVSpace,
                vaddr_word,
                rights,
                attr,
            )
        }
    }

    fn assert_page_aligned(vaddr: usize) {
        assert_eq!(
            vaddr & (PAGE_SIZE - 1),
            0,
            "virtual address 0x{vaddr:08x} must be page aligned",
        );
    }

    fn translation_indices(vaddr: usize) -> (usize, usize, usize, usize) {
        const MASK: usize = 0x1FF;
        const L1_SHIFT: usize = 39;
        const L2_SHIFT: usize = 30;
        const L3_SHIFT: usize = 21;
        const PAGE_SHIFT: usize = IPC_PAGE_BITS;

        let l1 = (vaddr >> L1_SHIFT) & MASK;
        let l2 = (vaddr >> L2_SHIFT) & MASK;
        let l3 = (vaddr >> L3_SHIFT) & MASK;
        let page = (vaddr >> PAGE_SHIFT) & MASK;
        (l1, l2, l3, page)
    }

    #[cfg(feature = "kernel")]
    fn guard_bootinfo_access(&self) {
        let header_addr = self.bootinfo as *const _ as usize;
        let header_ptr = header_addr as *const u8;
        let header_byte = unsafe { ptr::read_volatile(header_ptr) };

        let (extra_bytes, extra_start, extra_end, _) =
            match bootinfo_extra_slice(self.bootinfo, BOOTINFO_FRAME_BYTES) {
                Ok((bytes, start, end, limit)) => (bytes, start, end, limit),
                Err(err) => {
                    ::log::error!("[boot] bootinfo extra validation failed: {err}",);
                    crate::sel4::debug_halt();
                    return;
                }
            };

        debug_assert!(
            extra_bytes.is_empty() || extra_start < extra_end,
            "bootinfo extra range must be non-empty when len > 0"
        );

        ::log::trace!(
            "[boot] bootinfo header @ 0x{header_addr:08x} byte=0x{header_byte:02x} extra=[0x{extra_start:08x}..0x{extra_end:08x})",
        );

        if extra_bytes.is_empty() {
            ::log::warn!("[boot] bootinfo extra region empty; skipping guard probe");
            return;
        }

        let probe_offset = extra_bytes.len().saturating_sub(1);
        let probe_addr = extra_start + probe_offset;
        debug_assert!(probe_addr < extra_end);

        let probe_ptr = probe_addr as *const u8;
        let probe_byte = unsafe { ptr::read_volatile(probe_ptr) };
        ::log::trace!("[boot] bootinfo extra probe @ 0x{probe_addr:08x} byte=0x{probe_byte:02x}",);
    }

    #[cfg(not(feature = "kernel"))]
    fn guard_bootinfo_access(&self) {}

    #[inline(always)]
    fn mapping_already_present(err: seL4_Error) -> bool {
        err == sel4_sys::seL4_DeleteFirst || err == sel4_sys::seL4_IllegalOperation
    }

    #[inline(always)]
    fn is_device_window_vaddr(&self, vaddr: usize) -> bool {
        vaddr >= DEVICE_VADDR_BASE && vaddr < DMA_VADDR_BASE
    }

    fn reserve_device_page_table(
        &mut self,
        level: &'static str,
        vaddr: usize,
    ) -> Result<ReservedUntyped, seL4_Error> {
        let Some(pool) = self.device_pt_pool.as_mut() else {
            log::error!(
                "[device-pt] pool unavailable for level={level} vaddr=0x{vaddr:016x}; cannot reserve",
            );
            return Err(sel4_sys::seL4_NotEnoughMemory);
        };
        let before = pool.remaining_tables();
        assert!(
            before > 0,
            "device page-table pool exhausted before mapping level={level} vaddr=0x{vaddr:016x}",
        );
        let reserved = pool.reserve_page_table()?;
        self.untyped
            .record_usage(pool.index, pool.used_bytes as u128);
        log::debug!(
            "[device-pt] reserve level={level} vaddr=0x{vaddr:016x} remaining_tables={remaining}",
            remaining = pool.remaining_tables(),
        );
        Ok(reserved)
    }

    fn release_reserved_page_table(&mut self, reserved: &ReservedUntyped) {
        if let Some(pool) = self.device_pt_pool.as_mut() {
            if pool.matches_index(reserved.index) {
                pool.release(reserved);
                self.untyped
                    .record_usage(pool.index, pool.used_bytes as u128);
                return;
            }
        }

        self.untyped.release(reserved);
    }

    fn reserve_page_table_for_vaddr(
        &mut self,
        table_base: usize,
        mapping_vaddr: usize,
        level: &'static str,
    ) -> Result<ReservedUntyped, seL4_Error> {
        if self.is_device_window_vaddr(mapping_vaddr) {
            return self.reserve_device_page_table(level, table_base);
        }

        self.untyped
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)
    }

    fn ensure_page_table(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        self.ensure_page_directory(vaddr)?;
        let pt_base = PageTableBookkeeper::<MAX_PAGE_TABLES>::base_for(vaddr);
        if self.page_tables.contains_base(pt_base) {
            return Ok(());
        }

        let reserved = self.reserve_page_table_for_vaddr(pt_base, vaddr, "page_table")?;
        let pt_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pt_slot,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageTable { vaddr: pt_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.release_reserved_page_table(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);
        if self.ipcbuf_trace {
            crate::bp!("ipcbuf.pt.retype.ok");
        }

        let pt_base_word =
            sel4_sys::seL4_Word::try_from(pt_base).expect("page table base must fit in seL4_Word");
        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pt_slot,
                seL4_CapInitThreadVSpace,
                pt_base_word,
                seL4_ARM_Page_Default,
            )
        };
        if map_res == seL4_NoError {
            self.page_tables
                .remember_base(pt_base)
                .map_err(|_| seL4_NotEnoughMemory)?;
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.pt.map.ok");
            }
            return Ok(());
        }

        unsafe {
            let depth = self.bootinfo.init_cnode_depth();
            let _ = seL4_CNode_Delete(self.init_cnode_cap(), pt_slot as seL4_CPtr, depth.into());
        }
        // Retyping consumed the parent-untyped watermark. Deleting this
        // derived cap cannot rewind that watermark while sibling objects from
        // the same BootInfo untyped remain live, so the reservation must stay
        // consumed even when the kernel already supplied this table.

        if Self::mapping_already_present(map_res) {
            // The kernel may boot with intermediate tables already installed
            // for the selected root-task window. Keep final frame mappings
            // strict in `map_frame`; only the intermediate table collision is
            // accepted here.
            log::trace!(
                "[cohesix:root-task] page table already mapped @ 0x{base:08x}",
                base = pt_base
            );
            self.page_tables
                .remember_base(pt_base)
                .map_err(|_| seL4_NotEnoughMemory)?;
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.pt.map.ok");
            }
            return Ok(());
        }

        self.record_retype(trace, RetypeStatus::Err(map_res));
        let _ = crate::bootstrap::ktry("ipcbuf.pt.map", map_res as i32);
        Err(map_res)
    }

    fn ensure_page_directory(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        let pd_base = PageDirectoryBookkeeper::<MAX_PAGE_DIRECTORIES>::base_for(vaddr);
        if self.page_directories.contains_base(pd_base) {
            return Ok(());
        }

        self.ensure_page_upper_directory(vaddr)?;

        let reserved = self.reserve_page_table_for_vaddr(pd_base, vaddr, "page_directory")?;
        let pd_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pd_slot,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageDirectory { vaddr: pd_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.release_reserved_page_table(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);

        let pd_base_word = sel4_sys::seL4_Word::try_from(pd_base)
            .expect("page directory base must fit in seL4_Word");
        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pd_slot,
                seL4_CapInitThreadVSpace,
                pd_base_word,
                seL4_ARM_Page_Default,
            )
        };
        if map_res == seL4_NoError {
            self.page_directories
                .remember_base(pd_base)
                .map_err(|_| seL4_NotEnoughMemory)?;
            return Ok(());
        }

        unsafe {
            let depth = self.bootinfo.init_cnode_depth();
            let _ = seL4_CNode_Delete(self.init_cnode_cap(), pd_slot as seL4_CPtr, depth.into());
        }
        // The successful retype advanced the kernel watermark. Cap deletion
        // does not make these bytes reusable without revoking/resetting the
        // parent untyped, which is not safe while its siblings remain live.

        if Self::mapping_already_present(map_res) {
            // The final page mapping remains strict; this accepts only a
            // boot-seeded intermediate directory at the selected VSpace slot.
            log::trace!(
                "[cohesix:root-task] page directory already mapped @ 0x{base:08x}",
                base = pd_base
            );
            self.page_directories
                .remember_base(pd_base)
                .map_err(|_| seL4_NotEnoughMemory)?;
            return Ok(());
        }

        self.record_retype(trace, RetypeStatus::Err(map_res));
        Err(map_res)
    }

    fn ensure_page_upper_directory(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        let pud_base = PageUpperDirectoryBookkeeper::<MAX_PAGE_UPPER_DIRECTORIES>::base_for(vaddr);
        if self.page_upper_directories.contains_base(pud_base) {
            return Ok(());
        }

        let reserved =
            self.reserve_page_table_for_vaddr(pud_base, vaddr, "page_upper_directory")?;
        let pud_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pud_slot,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageUpperDirectory { vaddr: pud_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.release_reserved_page_table(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);

        let pud_base_word = sel4_sys::seL4_Word::try_from(pud_base)
            .expect("page upper directory base must fit in seL4_Word");
        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pud_slot,
                seL4_CapInitThreadVSpace,
                pud_base_word,
                seL4_ARM_Page_Default,
            )
        };
        if map_res == seL4_NoError {
            self.page_upper_directories
                .remember_base(pud_base)
                .map_err(|_| seL4_NotEnoughMemory)?;
            return Ok(());
        }

        unsafe {
            let depth = self.bootinfo.init_cnode_depth();
            let _ = seL4_CNode_Delete(self.init_cnode_cap(), pud_slot as seL4_CPtr, depth.into());
        }
        // Keep the software watermark aligned with seL4 after the successful
        // retype; deleting only this cap cannot reclaim its parent-untyped
        // allocation while other descendants survive.

        if Self::mapping_already_present(map_res) {
            // Treat a pre-existing intermediate directory as discovered boot
            // state, not as final frame aliasing.
            log::trace!(
                "[cohesix:root-task] page upper directory already mapped @ 0x{base:08x}",
                base = pud_base
            );
            self.page_upper_directories
                .remember_base(pud_base)
                .map_err(|_| seL4_NotEnoughMemory)?;
            return Ok(());
        }

        self.record_retype(trace, RetypeStatus::Err(map_res));
        Err(map_res)
    }

    fn prepare_retype_trace(
        &mut self,
        reserved: &ReservedUntyped,
        slot: seL4_CPtr,
        object_type: seL4_Word,
        object_size_bits: seL4_Word,
        kind: RetypeKind,
    ) -> RetypeTrace {
        // Target the root CNode directly and describe the destination slot explicitly.
        // seL4 resolves the `(root, node_index, node_depth)` triple to select the CNode that will
        // receive the new capability. Init-root retypes rely on the canonical
        // `(node_index = seL4_CapInitThreadCNode, node_depth = seL4_WordBits,
        // dest_offset = slot)` tuple so that the kernel resolves the root CNode
        // capability and then places the child cap at the slot offset.
        let cnode_root = self.bootinfo.init_cnode_cap();
        let node_index: seL4_Word = cspace_sys::init_root_index();
        let cnode_depth: seL4_Word = cspace_sys::canonical_depth_word();
        let dest_offset: seL4_Word = slot as seL4_Word;
        RetypeTrace {
            untyped_cap: reserved.cap(),
            untyped_paddr: reserved.paddr(),
            untyped_size_bits: reserved.size_bits(),
            cnode_root,
            dest_slot: slot,
            dest_offset,
            cnode_depth,
            node_index,
            object_type,
            object_size_bits,
            kind,
        }
    }

    fn log_retype_invocation(&self, trace: &RetypeTrace) {
        let init_cnode_cap = self.bootinfo.init_cnode_cap();
        let window = self.slots.window();
        let boot_first_free = self.bootinfo.empty_first_slot();
        log::trace!(
            "[cspace] window start=0x{start:04x} next=0x{next:04x} end=0x{end:04x} boot_first_free=0x{boot_first:04x} dest=0x{dest:04x}",
            start = window.start,
            next = window.next,
            end = window.end,
            boot_first = boot_first_free,
            dest = trace.dest_slot,
        );

        if trace.cnode_root == init_cnode_cap {
            log::trace!(
                "Retype → root=0x{:x} (initCNode) index=0x{:x} depth={} offset=0x{:x} (objtype={}({}), size_bits={}, untyped_paddr=0x{:08x})",
                trace.cnode_root,
                trace.node_index,
                trace.cnode_depth,
                trace.dest_offset,
                trace.object_type,
                objtype_name(trace.object_type),
                trace.object_size_bits,
                trace.untyped_paddr,
            );
        } else {
            log::trace!(
                "Retype → root=0x{:x} index=0x{:x} depth={} offset=0x{:x} (objtype={}({}), size_bits={}, untyped_paddr=0x{:08x})",
                trace.cnode_root,
                trace.node_index,
                trace.cnode_depth,
                trace.dest_offset,
                trace.object_type,
                objtype_name(trace.object_type),
                trace.object_size_bits,
                trace.untyped_paddr,
            );
        }
    }

    fn record_retype(&mut self, trace: RetypeTrace, status: RetypeStatus) {
        let init_cnode_cap = self.bootinfo.init_cnode_cap();
        let init_bits = self.bootinfo.init_cnode_bits();
        let expected_depth: seL4_Word = cspace_sys::canonical_depth_word();
        let expected_index: seL4_Word = cspace_sys::init_root_index();
        let expected_offset: seL4_Word = trace.dest_slot as seL4_Word;
        let max_slots = 1usize.checked_shl(init_bits as u32).unwrap_or_else(|| {
            panic!(
                "initThreadCNodeSizeBits {} exceeds host word size",
                init_bits
            )
        });

        let mut sanitise_error = None;
        let mut sanitised = None;

        if trace.cnode_root != init_cnode_cap {
            sanitise_error = Some(RetypeSanitiseError::RootMismatch {
                provided: trace.cnode_root,
                expected: init_cnode_cap,
            });
        } else if trace.cnode_depth != expected_depth {
            sanitise_error = Some(RetypeSanitiseError::DepthMismatch {
                provided: trace.cnode_depth,
                expected: expected_depth,
            });
        } else {
            let node_index = trace.node_index;
            if (node_index as usize) >= max_slots {
                sanitise_error = Some(RetypeSanitiseError::NodeIndexOutOfRange {
                    provided: node_index,
                    capacity: max_slots,
                });
            } else if node_index != expected_index {
                sanitise_error = Some(RetypeSanitiseError::NodeIndexMismatch {
                    provided: node_index,
                    expected: expected_index,
                });
            } else {
                let dest_offset = trace.dest_offset;
                if (dest_offset as usize) >= max_slots {
                    sanitise_error = Some(RetypeSanitiseError::OffsetOutOfRange {
                        provided: dest_offset,
                        capacity: max_slots,
                    });
                } else if dest_offset != expected_offset {
                    sanitise_error = Some(RetypeSanitiseError::DestOffsetMismatch {
                        offset: dest_offset,
                        slot: expected_offset,
                    });
                } else {
                    let mut sanitised_trace = trace;
                    sanitised_trace.cnode_root = init_cnode_cap;
                    sanitised_trace.node_index = expected_index;
                    sanitised_trace.cnode_depth = expected_depth;
                    sanitised_trace.dest_offset = expected_offset;
                    sanitised = Some(sanitised_trace);
                }
            }
        }

        if let RetypeStatus::Err(code) = status {
            if let Some(sanitised_trace) = sanitised {
                log::error!(
                    "[cohesix:root-task] retype.error: status={}({}) root=0x{:04x} index=0x{:04x} depth={} dest=0x{:04x} slot=0x{:04x} objtype={}({}) size_bits={} untyped_paddr=0x{:08x} kind={:?}",
                    error_name(code),
                    code,
                    sanitised_trace.cnode_root,
                    sanitised_trace.node_index,
                    sanitised_trace.cnode_depth,
                    sanitised_trace.dest_offset,
                    sanitised_trace.dest_slot,
                    sanitised_trace.object_type,
                    objtype_name(sanitised_trace.object_type),
                    sanitised_trace.object_size_bits,
                    sanitised_trace.untyped_paddr,
                    sanitised_trace.kind,
                );
            } else if let Some(reason) = sanitise_error {
                log::error!(
                    "[cohesix:root-task] retype.sanitise_error={reason} raw_root=0x{:04x} raw_index=0x{:04x} raw_depth={} raw_dest=0x{:04x} objtype={}({}) size_bits={} untyped_paddr=0x{:08x} kind={:?}",
                    trace.cnode_root,
                    trace.node_index,
                    trace.cnode_depth,
                    trace.dest_offset,
                    trace.object_type,
                    objtype_name(trace.object_type),
                    trace.object_size_bits,
                    trace.untyped_paddr,
                    trace.kind,
                );
            } else {
                log::error!(
                    "[cohesix:root-task] retype.error: status={}({}) raw_root=0x{:04x} raw_index=0x{:04x} raw_depth={} raw_dest=0x{:04x} objtype={}({}) size_bits={} untyped_paddr=0x{:08x} kind={:?}",
                    error_name(code),
                    code,
                    trace.cnode_root,
                    trace.node_index,
                    trace.cnode_depth,
                    trace.dest_offset,
                    trace.object_type,
                    objtype_name(trace.object_type),
                    trace.object_size_bits,
                    trace.untyped_paddr,
                    trace.kind,
                );
            }
        }

        self.last_retype = Some(RetypeLog {
            trace,
            init_cnode_cap,
            init_cnode_slot: init_cnode_cap,
            init_cnode_bits: init_bits,
            init_cnode_capacity: max_slots,
            canonical_cnode_depth: expected_depth,
            sanitised,
            sanitise_error,
            status,
        });
    }
}

#[derive(Clone)]
struct TranslationBookkeeper<const N: usize, const ALIGN: usize> {
    entries: Vec<usize, N>,
}

impl<const N: usize, const ALIGN: usize> TranslationBookkeeper<N, ALIGN> {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn base_for(vaddr: usize) -> usize {
        debug_assert!(ALIGN.is_power_of_two());
        vaddr & !(ALIGN - 1)
    }

    fn contains_base(&self, base: usize) -> bool {
        self.entries.iter().any(|&value| value == base)
    }

    fn contains(&self, vaddr: usize) -> bool {
        let base = Self::base_for(vaddr);
        self.contains_base(base)
    }

    fn remember_base(&mut self, base: usize) -> Result<(), ()> {
        if self.contains_base(base) {
            return Ok(());
        }
        self.entries.push(base).map_err(|_| ())
    }

    fn forget_base(&mut self, base: usize) {
        if let Some(position) = self.entries.iter().position(|&value| value == base) {
            let _ = self.entries.swap_remove(position);
        }
    }

    fn count(&self) -> usize {
        self.entries.len()
    }
}

type PageTableBookkeeper<const N: usize> = TranslationBookkeeper<N, PAGE_TABLE_ALIGN>;
type PageDirectoryBookkeeper<const N: usize> = TranslationBookkeeper<N, PAGE_DIRECTORY_ALIGN>;
type PageUpperDirectoryBookkeeper<const N: usize> =
    TranslationBookkeeper<N, PAGE_UPPER_DIRECTORY_ALIGN>;

/// Per-target-VSpace intermediate table tracker for isolated driver mappings.
#[cfg(feature = "kernel")]
#[derive(Clone)]
pub struct VSpaceTableTracker {
    page_tables: PageTableBookkeeper<MAX_DRIVER_VSPACE_PAGE_TABLES>,
    page_directories: PageDirectoryBookkeeper<MAX_DRIVER_VSPACE_PAGE_DIRECTORIES>,
    page_upper_directories: PageUpperDirectoryBookkeeper<MAX_DRIVER_VSPACE_PAGE_UPPER_DIRECTORIES>,
}

/// Failure while building a generation-revocable child VSpace.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevokeAnchorVSpaceError {
    /// A caller-supplied destination slot is null or duplicated.
    InvalidDestinationSlots,
    /// The fixed translation-object destination-slot bound was exhausted.
    TranslationObjectBound,
    /// seL4 rejected a retype, ASID assignment, mapping, or revoke operation.
    Sel4(seL4_Error),
}

impl From<seL4_Error> for RevokeAnchorVSpaceError {
    fn from(error: seL4_Error) -> Self {
        Self::Sel4(error)
    }
}

/// Caller-owned translation slots and mappings for one revoke-anchor generation.
///
/// Every slot must already have been removed from the ordinary slot allocator.
/// The same slots can be reused only through
/// [`KernelEnv::revoke_anchor_descendants_and_reset_vspace`], which performs a
/// successful kernel revoke before clearing this tracker's generation state.
#[cfg(feature = "kernel")]
#[derive(Clone)]
pub struct RevokeAnchorVSpaceTracker<const N: usize> {
    destination_slots: [seL4_CPtr; N],
    used_slots: usize,
    tables: VSpaceTableTracker,
}

#[cfg(feature = "kernel")]
impl<const N: usize> RevokeAnchorVSpaceTracker<N> {
    /// Bind an exact non-empty set of distinct caller-owned destination slots.
    pub fn new(destination_slots: [seL4_CPtr; N]) -> Result<Self, RevokeAnchorVSpaceError> {
        if N == 0 {
            return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
        }
        for (index, slot) in destination_slots.iter().copied().enumerate() {
            if slot == seL4_CapNull || destination_slots[..index].contains(&slot) {
                return Err(RevokeAnchorVSpaceError::InvalidDestinationSlots);
            }
        }
        Ok(Self {
            destination_slots,
            used_slots: 0,
            tables: VSpaceTableTracker::new(),
        })
    }

    /// Number of anchor-derived PUD/PD/PT objects installed this generation.
    #[must_use]
    pub fn mapped_table_count(&self) -> usize {
        self.tables.mapped_table_count()
    }

    /// Remaining exact translation-object capacity.
    #[must_use]
    pub const fn remaining_slots(&self) -> usize {
        N.saturating_sub(self.used_slots)
    }

    /// The fixed destination-slot set retained across generations.
    #[must_use]
    pub const fn destination_slots(&self) -> &[seL4_CPtr; N] {
        &self.destination_slots
    }

    fn take_destination_slot(&mut self) -> Result<seL4_CPtr, RevokeAnchorVSpaceError> {
        let slot = self
            .destination_slots
            .get(self.used_slots)
            .copied()
            .ok_or(RevokeAnchorVSpaceError::TranslationObjectBound)?;
        self.used_slots += 1;
        Ok(slot)
    }

    fn reset_after_revoke(&mut self) {
        self.used_slots = 0;
        self.tables = VSpaceTableTracker::new();
    }
}

#[cfg(feature = "kernel")]
impl VSpaceTableTracker {
    /// Creates an empty tracker for one isolated target VSpace.
    #[must_use]
    pub fn new() -> Self {
        Self {
            page_tables: PageTableBookkeeper::new(),
            page_directories: PageDirectoryBookkeeper::new(),
            page_upper_directories: PageUpperDirectoryBookkeeper::new(),
        }
    }

    /// Number of intermediate translation objects installed so far.
    #[must_use]
    pub fn mapped_table_count(&self) -> usize {
        self.page_tables.count()
            + self.page_directories.count()
            + self.page_upper_directories.count()
    }
}

#[cfg(feature = "kernel")]
impl Default for VSpaceTableTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_slot_bitmap_tracks_hundreds_of_exact_anchors() {
        let mut reserved = ReservedSlotBitmap::new();
        let first = ROOT_CSPACE_SLOT_CAPACITY - 384;
        for index in first..ROOT_CSPACE_SLOT_CAPACITY {
            let slot = seL4_CPtr::try_from(index).expect("root CSpace slot fits seL4 CPtr");
            assert_eq!(reserved.insert(slot), Ok(()));
            assert!(reserved.contains(slot));
        }
        let duplicate = seL4_CPtr::try_from(first).expect("root CSpace slot fits seL4 CPtr");
        assert_eq!(reserved.insert(duplicate), Err(sel4_sys::seL4_DeleteFirst));
        assert_eq!(
            reserved.insert(
                seL4_CPtr::try_from(ROOT_CSPACE_SLOT_CAPACITY)
                    .expect("root CSpace capacity fits seL4 CPtr"),
            ),
            Err(sel4_sys::seL4_RangeError)
        );
    }

    #[test]
    fn revoke_anchor_vspace_tracker_rejects_aliases_and_bounds_slots() {
        assert!(matches!(
            RevokeAnchorVSpaceTracker::<2>::new([41, 41]),
            Err(RevokeAnchorVSpaceError::InvalidDestinationSlots)
        ));
        let mut tracker = RevokeAnchorVSpaceTracker::new([41, 42])
            .expect("distinct caller-owned translation slots");
        assert_eq!(tracker.take_destination_slot(), Ok(41));
        assert_eq!(tracker.take_destination_slot(), Ok(42));
        assert_eq!(
            tracker.take_destination_slot(),
            Err(RevokeAnchorVSpaceError::TranslationObjectBound)
        );
        tracker.reset_after_revoke();
        assert_eq!(tracker.take_destination_slot(), Ok(41));
    }

    #[test]
    fn manual_initial_caps_marked_reserved() {
        let manual_caps: &[seL4_CPtr] = &[
            seL4_CapNull,
            seL4_CapInitThreadTCB,
            seL4_CapInitThreadCNode,
            seL4_CapInitThreadVSpace,
            seL4_CapIRQControl,
            seL4_CapASIDControl,
            seL4_CapInitThreadASIDPool,
            seL4_CapIOPort,
            seL4_CapIOSpace,
            seL4_CapBootInfoFrame,
            seL4_CapInitThreadIPCBuffer,
            seL4_CapDomain,
            seL4_CapSMMUSIDControl,
            seL4_CapSMMUCBControl,
            seL4_CapInitThreadSC,
        ];

        for &cap in manual_caps {
            assert!(
                is_boot_reserved_slot(cap),
                "cap 0x{cap:04x} should be reserved"
            );
        }

        assert!(is_boot_reserved_slot(seL4_CapSMC));
    }

    #[test]
    fn error_name_reports_expected_labels() {
        let cases: &[(seL4_Error, &str)] = &[
            (sel4_sys::seL4_NoError, "seL4_NoError"),
            (sel4_sys::seL4_InvalidArgument, "seL4_InvalidArgument"),
            (sel4_sys::seL4_InvalidCapability, "seL4_InvalidCapability"),
            (sel4_sys::seL4_IllegalOperation, "seL4_IllegalOperation"),
            (sel4_sys::seL4_RangeError, "seL4_RangeError"),
            (sel4_sys::seL4_AlignmentError, "seL4_AlignmentError"),
            (sel4_sys::seL4_FailedLookup, "seL4_FailedLookup"),
            (sel4_sys::seL4_TruncatedMessage, "seL4_TruncatedMessage"),
            (sel4_sys::seL4_DeleteFirst, "seL4_DeleteFirst"),
            (sel4_sys::seL4_RevokeFirst, "seL4_RevokeFirst"),
            (sel4_sys::seL4_NotEnoughMemory, "seL4_NotEnoughMemory"),
        ];

        for &(code, expected) in cases {
            assert_eq!(error_name(code), expected);
        }

        assert_eq!(error_name(42), "seL4_UnknownError");
    }

    #[test]
    fn unmapped_ram_frame_handle_exposes_no_root_mapping_pointer() {
        let frame = UnmappedRamFrame {
            cap: 0x42,
            paddr: 0x8000_0000,
        };

        assert_eq!(frame.cap(), 0x42);
        assert_eq!(frame.paddr(), 0x8000_0000);
    }

    #[test]
    fn root_device_cache_excludes_large_framebuffer_ram() {
        assert!(!root_device_frame_cache_eligible(0x3e51_3000));
        assert!(root_device_frame_cache_eligible(0xfe00_b000));
        assert!(root_device_frame_cache_eligible(0xfe20_0000));
        assert!(root_device_frame_cache_eligible(0xfe30_0000));
        assert!(root_device_frame_cache_eligible(0xfd50_0000));
        assert!(root_device_frame_cache_eligible(0x0000_0006_0000_0000));
    }

    #[test]
    fn child_device_page_admission_distinguishes_unmapped_and_root_mapped_caps() {
        let mut bootinfo = blank_bootinfo_for_tests();
        store_bootinfo_empty_region(
            &mut bootinfo.empty,
            0,
            1 << 13,
            "test.child_device_page_admission",
        );
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let mut env = KernelEnv::new(bootinfo_ref, None, ReservedVaddrRanges::new());
        let mailbox_paddr = 0xfe00_b000;
        let dma_paddr = 0xfe00_7000;

        assert!(!env.device_page_available_for_child(mailbox_paddr));
        assert!(!env.device_page_admitted_for_child_without_root_mapping(dma_paddr));
        assert!(env
            .device_frame_cache
            .push(DeviceFrameCacheEntry {
                paddr: mailbox_paddr,
                source_cap: 0x123,
                root_cap: Some(0x124),
                root_vaddr: Some(0xa000_0000),
                exclusive_child_admission: false,
            })
            .is_ok());
        assert!(env
            .device_frame_cache
            .push(DeviceFrameCacheEntry {
                paddr: dma_paddr,
                source_cap: 0x125,
                root_cap: None,
                root_vaddr: None,
                exclusive_child_admission: true,
            })
            .is_ok());

        assert!(env.device_page_available_for_child(mailbox_paddr));
        assert!(env.device_page_available_for_child(dma_paddr));
        assert!(!env.device_page_admitted_for_child_without_root_mapping(mailbox_paddr));
        assert!(env.device_page_admitted_for_child_without_root_mapping(dma_paddr));

        let mut tracker = VSpaceTableTracker::new();
        assert_eq!(
            env.map_device_page_into_vspace(
                dma_paddr,
                0x200,
                0x4000_0000,
                seL4_CapRights_ReadWrite,
                DEVICE_VM_ATTRIBUTES,
                &mut tracker,
            ),
            Err(sel4_sys::seL4_IllegalOperation),
        );
        assert_eq!(tracker.mapped_table_count(), 0);
    }

    #[test]
    fn aarch64_vspace_and_asid_objects_are_page_sized() {
        assert_eq!(
            objtype_name(SEL4_ARM_VSPACE_OBJECT_WORD),
            "seL4_ARM_VSpaceObject"
        );
        assert_eq!(
            object_type_name(sel4_sys::seL4_ARM_VSpaceObjectType),
            "seL4_ARM_VSpaceObject"
        );
        assert_eq!(sel4_sys::seL4_VSpaceBits as usize, PAGE_BITS);
        assert_eq!(sel4_sys::seL4_ASIDPoolBits as usize, PAGE_BITS);
    }

    #[test]
    fn boot_seeded_intermediate_mapping_errors_are_recognized() {
        assert!(KernelEnv::mapping_already_present(
            sel4_sys::seL4_DeleteFirst
        ));
        assert!(KernelEnv::mapping_already_present(
            sel4_sys::seL4_IllegalOperation
        ));
        assert!(!KernelEnv::mapping_already_present(
            sel4_sys::seL4_FailedLookup
        ));
        assert!(!KernelEnv::mapping_already_present(
            sel4_sys::seL4_InvalidArgument
        ));
    }

    #[test]
    fn page_table_alignment_matches_two_meg_regions() {
        let base0 = PageTableBookkeeper::<4>::base_for(0xA000_1234);
        assert_eq!(base0, 0xA000_0000);
        let base1 = PageTableBookkeeper::<4>::base_for(0xA020_1000);
        assert_eq!(base1, 0xA020_0000);
    }

    #[test]
    fn page_directory_alignment_matches_one_gib_regions() {
        let base0 = PageDirectoryBookkeeper::<2>::base_for(0x4000_1000);
        assert_eq!(base0, 0x4000_0000);
        let base1 = PageDirectoryBookkeeper::<2>::base_for(0x7FFF_FFFF);
        assert_eq!(base1, 0x4000_0000);
    }

    #[test]
    fn page_upper_directory_alignment_matches_512_gib_regions() {
        let addr = 0x0002_0000_1000usize;
        let base = PageUpperDirectoryBookkeeper::<2>::base_for(addr);
        assert_eq!(base, 0);
    }

    #[test]
    fn device_pool_allocation_stops_at_capacity() {
        let mut pool = DevicePtPool {
            ut_slot: 0x0f3,
            paddr: 0x4000_0000,
            size_bits: 16,
            index: 3,
            used_bytes: 0,
            total_bytes: 1 << 16,
        };

        let mut successes = 0;
        while pool.remaining_tables() > 0 {
            pool.reserve_page_table()
                .expect("reservation within capacity");
            successes += 1;
        }

        assert_eq!(successes, (pool.total_bytes / (1 << PAGE_TABLE_BITS)));
        assert_eq!(pool.reserve_page_table(), Err(seL4_NotEnoughMemory));
    }

    #[test]
    fn reserve_ram_high_prefers_top_of_untyped() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0;
        bootinfo.untyped.end = 2;
        bootinfo.untypedList[0].paddr = 0x4000_0000;
        bootinfo.untypedList[0].sizeBits = 16;
        bootinfo.untypedList[0].isDevice = 0;
        bootinfo.untypedList[1].paddr = 0x5000_0000;
        bootinfo.untypedList[1].sizeBits = 16;
        bootinfo.untypedList[1].isDevice = 0;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        let high = catalog
            .reserve_ram_high(PAGE_BITS as u8)
            .expect("high reservation");

        assert_eq!(high.paddr(), 0x5000_0000);
    }

    #[test]
    fn release_restores_exact_pre_alignment_watermark() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0;
        bootinfo.untyped.end = 1;
        bootinfo.untypedList[0].paddr = 0x4000_0000;
        bootinfo.untypedList[0].sizeBits = 16;
        bootinfo.untypedList[0].isDevice = 0;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        catalog.record_usage(0, 0x800);
        let aligned = catalog.reserve_ram(PAGE_BITS as u8).expect("aligned page");
        assert_eq!(aligned.offset_bytes(), 0x1000);
        catalog.release(&aligned);

        let next = catalog.reserve_ram(11).expect("reservation after rollback");
        assert_eq!(next.offset_bytes(), 0x800);
        assert_eq!(next.paddr(), 0x4000_0800);
    }

    #[test]
    fn consumed_translation_page_is_not_reused_after_cap_deletion() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0x300;
        bootinfo.untyped.end = 0x302;
        bootinfo.untypedList[0].paddr = 0x4023_a000;
        bootinfo.untypedList[0].sizeBits = 13;
        bootinfo.untypedList[0].isDevice = 0;
        bootinfo.untypedList[1].paddr = 0x4023_c000;
        bootinfo.untypedList[1].sizeBits = 14;
        bootinfo.untypedList[1].isDevice = 0;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        catalog.record_usage(0, PAGE_SIZE as u128);
        let boot_collision = catalog
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .expect("trial table reservation");
        assert_eq!(boot_collision.paddr(), 0x4023_b000);

        // A successful retype consumed this reservation. Deleting only the
        // derived cap after a boot-seeded map collision must not release it.
        let next = catalog
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .expect("next table reservation");
        assert_eq!(next.paddr(), 0x4023_c000);
    }

    #[test]
    fn reserve_device_prefers_closest_covered_untyped() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0x300;
        bootinfo.untyped.end = 0x302;
        bootinfo.untypedList[0].paddr = 0x3c00_0000;
        bootinfo.untypedList[0].sizeBits = 26;
        bootinfo.untypedList[0].isDevice = 1;
        bootinfo.untypedList[1].paddr = 0x3e50_0000;
        bootinfo.untypedList[1].sizeBits = 21;
        bootinfo.untypedList[1].isDevice = 1;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        let reserved = catalog
            .reserve_device(0x3e51_3000, PAGE_BITS)
            .expect("device reservation should succeed");

        assert_eq!(reserved.cap(), 0x301);
        assert_eq!(reserved.paddr(), 0x3e50_0000);
    }

    #[test]
    fn device_coverage_prefers_closest_covered_untyped() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0x300;
        bootinfo.untyped.end = 0x302;
        bootinfo.untypedList[0].paddr = 0x3c00_0000;
        bootinfo.untypedList[0].sizeBits = 26;
        bootinfo.untypedList[0].isDevice = 1;
        bootinfo.untypedList[1].paddr = 0x3e50_0000;
        bootinfo.untypedList[1].sizeBits = 21;
        bootinfo.untypedList[1].isDevice = 1;

        let catalog = UntypedCatalog::new(&bootinfo, None);
        let coverage = catalog
            .device_coverage(0x3e51_3000, PAGE_BITS)
            .expect("coverage should resolve");

        assert_eq!(coverage.index, 1);
        assert_eq!(coverage.base, 0x3e50_0000);
        assert_eq!(coverage.limit, 0x3e70_0000);
    }

    #[test]
    fn device_coverage_disappears_after_cursor_advances_past_target_page() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0x300;
        bootinfo.untyped.end = 0x301;
        bootinfo.untypedList[0].paddr = 0xfd50_0000;
        bootinfo.untypedList[0].sizeBits = 16;
        bootinfo.untypedList[0].isDevice = 1;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        assert!(catalog.device_coverage(0xfd50_8000, PAGE_BITS).is_some());

        catalog.record_usage(0, 0xa000);
        assert!(catalog.device_coverage(0xfd50_8000, PAGE_BITS).is_none());
        assert!(catalog.device_coverage(0xfd50_9000, PAGE_BITS).is_none());
        assert!(catalog.device_coverage(0xfd50_a000, PAGE_BITS).is_some());
    }

    #[test]
    fn sdio_dma_page_must_be_admitted_before_higher_mailbox_page() {
        const DEVICE_BASE: usize = 0xfe00_0000;
        const DMA_PAGE: usize = 0xfe00_7000;
        const MAILBOX_PAGE: usize = 0xfe00_b000;

        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0x300;
        bootinfo.untyped.end = 0x301;
        bootinfo.untypedList[0].paddr = DEVICE_BASE as u64;
        bootinfo.untypedList[0].sizeBits = 21;
        bootinfo.untypedList[0].isDevice = 1;

        let mut ascending = UntypedCatalog::new(&bootinfo, None);
        assert!(ascending.device_coverage(DMA_PAGE, PAGE_BITS).is_some());
        ascending.record_usage(0, (DMA_PAGE - DEVICE_BASE + PAGE_SIZE) as u128);
        assert!(ascending.device_coverage(MAILBOX_PAGE, PAGE_BITS).is_some());

        let mut reversed = UntypedCatalog::new(&bootinfo, None);
        reversed.record_usage(0, (MAILBOX_PAGE - DEVICE_BASE + PAGE_SIZE) as u128);
        assert!(reversed.device_coverage(DMA_PAGE, PAGE_BITS).is_none());
    }

    #[test]
    fn reserve_paddr_range_consumes_prefix() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0;
        bootinfo.untyped.end = 1;
        bootinfo.untypedList[0].paddr = 0x4000_0000;
        bootinfo.untypedList[0].sizeBits = 16;
        bootinfo.untypedList[0].isDevice = 0;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        catalog.reserve_paddr_range(0x4000_0000..0x4000_3000, "test");

        let reserved = catalog
            .reserve_ram_high(PAGE_BITS as u8)
            .expect("reservation after prefix");
        assert_eq!(reserved.paddr(), 0x4000_3000);
    }

    #[test]
    fn reserve_paddr_range_mid_span_disables_entry() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.untyped.start = 0;
        bootinfo.untyped.end = 1;
        bootinfo.untypedList[0].paddr = 0x4000_0000;
        bootinfo.untypedList[0].sizeBits = 16;
        bootinfo.untypedList[0].isDevice = 0;

        let mut catalog = UntypedCatalog::new(&bootinfo, None);
        catalog.reserve_paddr_range(0x4000_1000..0x4000_2000, "test");

        assert!(catalog.reserve_ram_high(PAGE_BITS as u8).is_none());
    }

    #[test]
    fn header_bytes_span_entire_struct() {
        let bootinfo: seL4_BootInfo = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        let header = bootinfo.header_bytes();
        assert_eq!(header.len(), mem::size_of::<seL4_BootInfo>());
    }

    #[test]
    fn extra_bytes_returns_region_after_bootinfo_frame() {
        use core::mem::MaybeUninit;

        const EXTRA_BYTES: usize = 2 * mem::size_of::<seL4_Word>();
        const FRAME_GAP_BYTES: usize = BOOTINFO_FRAME_BYTES - mem::size_of::<seL4_BootInfo>();

        #[repr(C)]
        struct Fixture<const N: usize> {
            bootinfo: seL4_BootInfo,
            frame_gap: [u8; FRAME_GAP_BYTES],
            extra: [u8; N],
        }

        let mut fixture: Fixture<EXTRA_BYTES> = unsafe { MaybeUninit::zeroed().assume_init() };

        for byte in fixture.frame_gap.iter_mut() {
            *byte = 0xa5;
        }
        for (index, byte) in fixture.extra.iter_mut().enumerate() {
            *byte = index as u8;
        }

        fixture.bootinfo.extraLen = EXTRA_BYTES as seL4_Word;

        let extra = fixture.bootinfo.extra_bytes();
        assert_eq!(extra, &fixture.extra);
    }

    #[test]
    fn snapshot_view_uses_compact_extra_layout() {
        use core::mem::MaybeUninit;

        const EXTRA_BYTES: usize = 3 * mem::size_of::<seL4_Word>();
        const FRAME_GAP_BYTES: usize = BOOTINFO_FRAME_BYTES - mem::size_of::<seL4_BootInfo>();

        #[repr(C)]
        struct SourceFixture<const N: usize> {
            bootinfo: seL4_BootInfo,
            frame_gap: [u8; FRAME_GAP_BYTES],
            extra: [u8; N],
        }

        let mut source: SourceFixture<EXTRA_BYTES> = unsafe { MaybeUninit::zeroed().assume_init() };
        for byte in source.frame_gap.iter_mut() {
            *byte = 0xa5;
        }
        for (index, byte) in source.extra.iter_mut().enumerate() {
            *byte = (index as u8).wrapping_add(1);
        }
        source.bootinfo.extraLen = EXTRA_BYTES as seL4_Word;
        let source = Box::leak(Box::new(source));

        let source_view = BootInfoView::new(&source.bootinfo).expect("source bootinfo view");
        let mut snapshot_backing = [0u8; mem::size_of::<seL4_BootInfo>() + EXTRA_BYTES];
        snapshot_backing[..mem::size_of::<seL4_BootInfo>()]
            .copy_from_slice(source_view.header_bytes());
        snapshot_backing[mem::size_of::<seL4_BootInfo>()..].copy_from_slice(source_view.extra());

        let snapshot_header = unsafe { &*(snapshot_backing.as_ptr() as *const seL4_BootInfo) };
        let snapshot_view =
            BootInfoView::from_snapshot_source(&source_view, snapshot_header).expect("snapshot");
        let expected_start = snapshot_header as *const _ as usize + mem::size_of::<seL4_BootInfo>();
        let expected_end = expected_start + EXTRA_BYTES;

        assert_eq!(snapshot_view.extra_range().start, expected_start);
        assert_eq!(snapshot_view.extra_range().end, expected_end);
        assert_eq!(
            snapshot_view.extra(),
            &snapshot_backing[mem::size_of::<seL4_BootInfo>()..]
        );
    }

    #[test]
    fn remember_base_deduplicates_entries() {
        let mut keeper: PageTableBookkeeper<2> = PageTableBookkeeper::new();
        let base = PageTableBookkeeper::<2>::base_for(0x1000);
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.contains_base(base));
        assert_eq!(keeper.count(), 1);
    }

    #[test]
    fn remember_base_respects_capacity() {
        let mut keeper: PageTableBookkeeper<1> = PageTableBookkeeper::new();
        let base0 = PageTableBookkeeper::<1>::base_for(0x0);
        let base1 = PageTableBookkeeper::<1>::base_for(PAGE_TABLE_ALIGN);
        assert!(keeper.remember_base(base0).is_ok());
        assert!(keeper.remember_base(base1).is_err());
        assert!(keeper.contains_base(base0));
        assert_eq!(keeper.count(), 1);
    }

    #[test]
    fn device_mappings_use_uncached_execute_never_attributes() {
        assert_eq!(
            vm_attributes_raw(DEVICE_VM_ATTRIBUTES),
            vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever)
        );
        assert_eq!(
            vm_attributes_raw(DEVICE_VM_ATTRIBUTES) & vm_attributes_raw(seL4_ARM_Page_Default),
            0,
            "device MMIO must not request cacheable normal-memory attributes",
        );
    }

    #[test]
    fn canonical_cnode_bits_accepts_word_width() {
        let mut bi = blank_bootinfo_for_tests();
        bi.initThreadCNodeSizeBits = sel4_sys::seL4_WordBits as u8;
        bi.empty.end = 1;

        let bits = canonical_cnode_bits(&bi);
        assert_eq!(bits, sel4_sys::seL4_WordBits as u8);
    }

    #[test]
    fn canonical_cnode_bits_clamps_overflow() {
        let mut bi = blank_bootinfo_for_tests();
        bi.initThreadCNodeSizeBits = u8::MAX;
        bi.empty.end = 0;

        let bits = canonical_cnode_bits(&bi);
        assert_eq!(bits, sel4_sys::seL4_WordBits as u8);
    }

    #[test]
    fn contains_uses_alignment_when_tracking() {
        let mut keeper: PageTableBookkeeper<4> = PageTableBookkeeper::new();
        let base = PageTableBookkeeper::<4>::base_for(0xA000_0000);
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.contains(0xA000_0ABC));
        assert!(keeper.contains(0xA001_FFFF));
        assert!(!keeper.contains(0xA020_0000));
    }

    #[test]
    fn retype_trace_targets_root_cnode_slot() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        store_bootinfo_empty_region(&mut bootinfo.empty, 0, 1 << 13, "test.retype_trace_root");
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let mut env = KernelEnv::new(bootinfo_ref, None, ReservedVaddrRanges::new());
        let reserved = ReservedUntyped {
            cap: 0x200,
            paddr: 0,
            previous_used_bytes: 0,
            offset_bytes: 0,
            size_bits: PAGE_BITS as u8,
            index: 0,
            reserved_bytes: 1 << PAGE_BITS,
        };
        let slot: seL4_CPtr = 0x00c8;
        let trace = env.prepare_retype_trace(
            &reserved,
            slot,
            SEL4_ARM_PAGE_OBJECT_WORD,
            PAGE_BITS as seL4_Word,
            RetypeKind::DevicePage { paddr: 0 },
        );
        assert_eq!(trace.cnode_root, bootinfo_ref.init_cnode_cap());
        let expected_index: seL4_Word = cspace_sys::init_root_index();
        let expected_depth: seL4_Word = bootinfo_ref.init_cnode_depth() as seL4_Word;
        assert_eq!(trace.node_index, expected_index);
        assert_eq!(trace.cnode_depth, expected_depth);
        assert_eq!(trace.dest_offset, slot as seL4_Word);
        assert_eq!(trace.dest_slot, slot);
    }

    #[test]
    fn retype_sanitiser_uses_canonical_depth() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        store_bootinfo_empty_region(&mut bootinfo.empty, 0, 1 << 13, "test.retype_sanitiser");
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let mut env = KernelEnv::new(bootinfo_ref, None, ReservedVaddrRanges::new());
        let dummy = ReservedUntyped {
            cap: 0x555,
            paddr: 0,
            previous_used_bytes: 0,
            offset_bytes: 0,
            size_bits: PAGE_TABLE_BITS as u8,
            index: 0,
            reserved_bytes: 1 << PAGE_TABLE_BITS,
        };
        let slot: seL4_CPtr = 0x00a2;
        let trace = env.prepare_retype_trace(
            &dummy,
            slot,
            seL4_ARM_PageTableObject as seL4_Word,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageTable { vaddr: 0 },
        );
        let (sanitised, init_bits) = env.sanitise_retype_trace(trace);
        assert_eq!(init_bits, 13);
        assert_eq!(
            sanitised.cnode_depth,
            bootinfo_ref.init_cnode_depth() as seL4_Word
        );
        assert_eq!(sanitised.node_index, cspace_sys::init_root_index());
        assert_eq!(sanitised.dest_offset, slot as seL4_Word);
    }

    #[test]
    fn bootinfo_capacity_bits_drive_cspace_math() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.initThreadCNodeSizeBits = 13;
        let init_bits = bootinfo.init_cnode_bits();
        assert_eq!(init_bits, 13);

        let capacity = 1usize << init_bits;
        assert_eq!(capacity, 8192);

        let empty_start = 0x00c8usize;
        let empty_end = 0x2000usize;
        assert!(empty_start < empty_end);
        assert!(empty_end <= capacity);
    }

    #[test]
    fn retype_bounds_use_bootinfo_bits_not_path_depth() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        store_bootinfo_empty_region(&mut bootinfo.empty, 0, 1 << 13, "test.retype_bounds");
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref, None, ReservedVaddrRanges::new());
        let init_root = bootinfo_ref.init_cnode_cap();

        let slot: seL4_CPtr = 0x00c8;
        let expected_depth: seL4_Word = bootinfo_ref.init_cnode_depth() as seL4_Word;
        let canonical_index: seL4_Word = cspace_sys::init_root_index();
        let trace = RetypeTrace {
            untyped_cap: 0x200,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: init_root,
            dest_slot: slot,
            dest_offset: slot as seL4_Word,
            cnode_depth: expected_depth,
            node_index: canonical_index,
            object_type: SEL4_ARM_PAGE_OBJECT_WORD,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DevicePage { paddr: 0 },
        };

        let (_, init_bits) = env.sanitise_retype_trace(trace);
        let max_slots = 1usize << init_bits;
        assert_eq!(init_bits, env.bootinfo().init_cnode_bits());
        assert!((slot as usize) < max_slots);
    }

    #[test]
    fn retype_trace_is_root_slot() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        store_bootinfo_empty_region(
            &mut bootinfo.empty,
            0,
            1 << 13,
            "test.retype_trace_root_slot",
        );
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref, None, ReservedVaddrRanges::new());
        let init_root = bootinfo_ref.init_cnode_cap();

        let slot: seL4_CPtr = 0x0097;
        let canonical_index: seL4_Word = cspace_sys::init_root_index();
        let expected_depth: seL4_Word = bootinfo_ref.init_cnode_depth() as seL4_Word;
        let trace = RetypeTrace {
            untyped_cap: 0x100,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: init_root,
            dest_slot: slot,
            dest_offset: slot as seL4_Word,
            cnode_depth: expected_depth,
            node_index: canonical_index,
            object_type: SEL4_ARM_PAGE_OBJECT_WORD,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DevicePage { paddr: 0 },
        };

        let (sanitised, init_bits) = env.sanitise_retype_trace(trace);
        assert_eq!(sanitised.node_index, canonical_index);
        assert_eq!(sanitised.cnode_depth, expected_depth);
        assert_eq!(sanitised.dest_offset, slot as seL4_Word);
        assert_eq!(init_bits, bootinfo_ref.init_cnode_bits());
    }

    #[test]
    fn sanitise_retype_trace_validates_offset_against_init_bits() {
        use std::panic::{self, AssertUnwindSafe};

        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        store_bootinfo_empty_region(
            &mut bootinfo.empty,
            0,
            1 << 13,
            "test.retype_trace_validate",
        );
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref, None, ReservedVaddrRanges::new());
        let init_root = bootinfo_ref.init_cnode_cap();
        let expected_depth: seL4_Word = bootinfo_ref.init_cnode_depth() as seL4_Word;
        let valid_trace = RetypeTrace {
            untyped_cap: 0x100,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: init_root,
            dest_slot: 0x1ff,
            dest_offset: 0x1ff,
            cnode_depth: expected_depth,
            node_index: init_root as seL4_Word,
            object_type: SEL4_ARM_PAGE_OBJECT_WORD,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DmaPage { paddr: 0 },
        };

        let (_, init_bits) = env.sanitise_retype_trace(valid_trace);
        assert_eq!(init_bits, 13);

        let mut invalid_index = valid_trace;
        invalid_index.node_index = (1 << 13) as seL4_Word;
        let index_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(invalid_index);
        }));
        assert!(index_check.is_err());

        let mut nonmatching_index = valid_trace;
        nonmatching_index.node_index = 1;
        let nonzero_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(nonmatching_index);
        }));
        assert!(nonzero_check.is_err());

        let mut invalid_depth = valid_trace;
        invalid_depth.cnode_depth = 1;
        let depth_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(invalid_depth);
        }));
        assert!(depth_check.is_err());

        let mut invalid_offset = valid_trace;
        invalid_offset.dest_offset = (1 << 13) as seL4_Word;
        let offset_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(invalid_offset);
        }));
        assert!(offset_check.is_err());

        let mut mismatch = valid_trace;
        mismatch.dest_offset = valid_trace.dest_offset.saturating_add(1);
        let mismatch_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(mismatch);
        }));
        assert!(mismatch_check.is_err());
    }

    #[test]
    fn bootinfo_window_guard_rejects_ascii_like_bounds() {
        use std::panic::{self, AssertUnwindSafe};

        crate::debug::clear_watches();
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        store_bootinfo_empty_region(
            &mut bootinfo.empty,
            0x100,
            0x200,
            "test.bootinfo_guard.initial",
        );
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let guard = BootinfoWindowGuard::new();
        guard.arm(bootinfo_ref);
        store_bootinfo_empty_start(
            &mut bootinfo_ref.empty,
            0x5b205d74656e3a3a,
            "test.bootinfo_guard.corrupt_start",
        );
        store_bootinfo_empty_end(
            &mut bootinfo_ref.empty,
            0x736e6f632d74656e,
            "test.bootinfo_guard.corrupt_end",
        );
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            guard.check("test.marker");
        }));
        assert!(result.is_err(), "guard must panic on window corruption");
    }
}
