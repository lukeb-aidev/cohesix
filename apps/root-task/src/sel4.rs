// Author: Lukas Bower
//! seL4 resource management helpers for the root task.
#![cfg(any(test, feature = "kernel"))]
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]
#![allow(unsafe_code)]

use core::{
    arch::asm,
    fmt, mem,
    ptr::{self, NonNull},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::bootstrap::cspace_sys;
use crate::bootstrap::ipcbuf_view::IpcBufView;
#[cfg(feature = "kernel")]
use crate::bootstrap::ktry;
use crate::serial;
use heapless::Vec;
pub use sel4_sys::{
    seL4_AllRights, seL4_CNode, seL4_CNode_Copy, seL4_CNode_Delete, seL4_CNode_Mint,
    seL4_CNode_Move, seL4_CPtr, seL4_CapASIDControl, seL4_CapBootInfoFrame, seL4_CapDomain,
    seL4_CapIOPortControl, seL4_CapIOSpace, seL4_CapIRQControl, seL4_CapInitThreadASIDPool,
    seL4_CapInitThreadCNode, seL4_CapInitThreadIPCBuffer, seL4_CapInitThreadSC,
    seL4_CapInitThreadTCB, seL4_CapInitThreadVSpace, seL4_CapNull, seL4_CapRights,
    seL4_CapRights_All, seL4_CapRights_ReadWrite, seL4_CapSMC, seL4_CapSMMUCBControl,
    seL4_CapSMMUSIDControl, seL4_Error, seL4_GetBootInfo, seL4_MessageInfo, seL4_NoError,
    seL4_NotEnoughMemory, seL4_RangeError, seL4_Untyped, seL4_Untyped_Retype, seL4_Word,
};

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

/// Returns the architectural word width (in bits) exposed by seL4.
#[inline(always)]
pub const fn word_bits() -> seL4_Word {
    WORD_BITS
}

use sel4_sys::{
    seL4_ARM_PageTableObject, seL4_ARM_PageTable_Map, seL4_ARM_Page_Default, seL4_ARM_Page_Map,
    seL4_ARM_Page_Uncached, seL4_ARM_VMAttributes, seL4_BootInfo, seL4_ObjectType, seL4_SlotRegion,
    UntypedDesc, MAX_BOOTINFO_UNTYPEDS,
};

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use sel4_panicking::write_debug_byte;

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use sel4_panicking::DebugSink;

/// Alias to the boot information structure exposed by `sel4_sys`.
pub type BootInfo = seL4_BootInfo;

const BOOTINFO_ALIGN_MASK: usize = 0xF;
const MAX_BOOTINFO_EXTRA_WORDS: usize = 32 * 1024;

/// Errors raised while validating a bootinfo pointer and its extra region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootInfoError {
    /// The supplied bootinfo pointer was null.
    Null,
    /// The bootinfo pointer was not aligned to the required boundary.
    Unaligned {
        /// Offending bootinfo pointer address supplied by the caller.
        address: usize,
    },
    /// The reported extra bootinfo span exceeded the permitted limit.
    ExtraTooLarge {
        /// Number of 64-bit words advertised for the extra bootinfo region.
        words: usize,
    },
    /// Arithmetic overflow occurred while computing bounds.
    Overflow,
    /// The computed extra range wrapped or was otherwise invalid.
    ExtraRange {
        /// Starting address of the invalid bootinfo extra range.
        start: usize,
        /// End address of the invalid bootinfo extra range.
        end: usize,
    },
}

impl fmt::Display for BootInfoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "bootinfo pointer was null"),
            Self::Unaligned { address } => {
                write!(f, "bootinfo pointer not 16-byte aligned: 0x{address:016x}")
            }
            Self::ExtraTooLarge { words } => {
                write!(f, "bootinfo.extraLen exceeded limit: {words} words")
            }
            Self::Overflow => write!(f, "bootinfo bounds computation overflowed"),
            Self::ExtraRange { start, end } => write!(
                f,
                "bootinfo extra range invalid: [0x{start:016x}..0x{end:016x})"
            ),
        }
    }
}

fn bootinfo_extra_slice<'a>(header: &'a seL4_BootInfo) -> Result<&'a [u8], BootInfoError> {
    let addr = header as *const _ as usize;
    if addr & BOOTINFO_ALIGN_MASK != 0 {
        return Err(BootInfoError::Unaligned { address: addr });
    }

    let extra_words = header.extraLen as usize;
    if extra_words == 0 {
        return Ok(&[]);
    }

    if extra_words > MAX_BOOTINFO_EXTRA_WORDS {
        return Err(BootInfoError::ExtraTooLarge { words: extra_words });
    }

    let word_bytes = mem::size_of::<seL4_Word>();
    let extra_len = extra_words
        .checked_mul(word_bytes)
        .ok_or(BootInfoError::Overflow)?;

    let header_size = mem::size_of::<seL4_BootInfo>();
    let extra_start = addr
        .checked_add(header_size)
        .ok_or(BootInfoError::Overflow)?;
    let extra_end = extra_start
        .checked_add(extra_len)
        .ok_or(BootInfoError::Overflow)?;

    if extra_end <= extra_start {
        return Err(BootInfoError::ExtraRange {
            start: extra_start,
            end: extra_end,
        });
    }

    // SAFETY: The kernel guarantees that bootinfo and its extra region are mapped as
    // readable memory for the root task. The calculations above ensure we do not
    // wrap the address space or overrun the reported length.
    let slice = unsafe { core::slice::from_raw_parts(extra_start as *const u8, extra_len) };
    Ok(slice)
}

/// Immutable projection of the kernel-supplied bootinfo region.
#[derive(Clone, Copy)]
pub struct BootInfoView {
    header: &'static seL4_BootInfo,
    extra_bytes: &'static [u8],
}

impl BootInfoView {
    fn build(header: &'static seL4_BootInfo) -> Result<Self, BootInfoError> {
        let extra_bytes = bootinfo_extra_slice(header)?;
        Ok(Self {
            header,
            extra_bytes,
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
    pub fn extra_words(&self) -> usize {
        self.header.extraLen as usize
    }

    /// Returns the radix width (in bits) of the init thread's CNode.
    #[must_use]
    pub fn init_cnode_bits(&self) -> u8 {
        self.header.initThreadCNodeSizeBits as u8
    }

    /// Returns the radix width of the init thread's CNode as `usize`.
    #[must_use]
    pub fn init_cnode_size_bits(&self) -> usize {
        self.header.initThreadCNodeSizeBits as usize
    }

    /// Returns the inclusive-exclusive slot range advertised as free by the kernel.
    #[must_use]
    pub fn init_cnode_empty_range(&self) -> (seL4_CPtr, seL4_CPtr) {
        (
            self.header.empty.start as seL4_CPtr,
            self.header.empty.end as seL4_CPtr,
        )
    }

    /// Returns the capability designating the init thread's root CNode.
    #[must_use]
    pub fn root_cnode_cap(&self) -> seL4_CPtr {
        sel4_sys::seL4_CapInitThreadCNode
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
    debug_assert!(ep != seL4_CapNull, "endpoint slot must be non-null");
    ROOT_ENDPOINT.store(ep as usize, Ordering::Release);
}

/// Clear the root endpoint pointer. Intended for tests.
#[inline]
pub fn clear_ep() {
    ROOT_ENDPOINT.store(0, Ordering::Release);
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

/// Issues an seL4 send only when the endpoint capability is initialised.
#[inline(never)]
pub fn send_guarded(info: seL4_MessageInfo) -> Result<(), IpcError> {
    let endpoint = ensure_endpoint()?;
    debug_assert_ne!(
        endpoint, seL4_CapNull,
        "send_guarded must not transmit on the null endpoint"
    );
    if !SEND_LOGGED.swap(true, Ordering::AcqRel) {
        log::info!("bootstrap: send on ep slot=0x{slot:04x}", slot = endpoint,);
    }
    unsafe { sel4_sys::seL4_Send(endpoint, info) };
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
    let m0 = mr0.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let m1 = mr1.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let m2 = mr2.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let m3 = mr3.map_or(ptr::null_mut(), |mr| mr as *mut seL4_Word);
    let info = unsafe { sel4_sys::seL4_CallWithMRs(endpoint, info, m0, m1, m2, m3) };
    Ok(info)
}

/// Issues an seL4 reply+receive cycle only when the endpoint is initialised.
#[inline(never)]
pub fn replyrecv_guarded(
    info: seL4_MessageInfo,
    badge: Option<&mut seL4_Word>,
) -> Result<seL4_MessageInfo, IpcError> {
    let endpoint = ensure_endpoint()?;
    let badge_ptr = badge.map_or(ptr::null_mut(), |b| b as *mut seL4_Word);

    #[allow(non_snake_case)]
    unsafe extern "C" {
        fn seL4_ReplyRecv(
            dest: seL4_CPtr,
            msg_info: seL4_MessageInfo,
            sender_badge: *mut seL4_Word,
        ) -> seL4_MessageInfo;
    }

    let message = unsafe { seL4_ReplyRecv(endpoint, info, badge_ptr) };
    Ok(message)
}

/// Returns the radix depth (in bits) advertised for the init thread's root CNode.
///
/// seL4 models the depth parameter as the number of address bits to consume when
/// traversing a CSpace path. For the init thread's single-level CSpace this
/// value must match `initThreadCNodeSizeBits`, ensuring that invocations such as
/// `seL4_CNode_Copy` and `seL4_CNode_Delete` address slots directly without
/// relying on guard bits.
#[inline]
pub fn init_cnode_depth(bi: &seL4_BootInfo) -> u8 {
    bi.initThreadCNodeSizeBits as u8
}

/// Emits a single byte to the seL4 debug console.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn debug_put_char(ch: i32) {
    unsafe { seL4_DebugPutChar(ch as u8) }
}

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
/// Installs the kernel-backed debug sink so that panic messages surface on the seL4 console.
#[inline(always)]
pub fn install_debug_sink() {
    unsafe extern "C" fn emit(_ctx: *mut (), byte: u8) {
        unsafe {
            seL4_DebugPutChar(byte);
        }
    }

    let sink = DebugSink {
        context: core::ptr::null_mut(),
        emit,
    };
    let emit_addr = sink.emit as usize;
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        line,
        "[sel4::install_debug_sink] emit=0x{emit:016x}",
        emit = emit_addr,
    );
    serial::puts(line.as_str());
    if emit_addr & 0b11 != 0 {
        panic!(
            "debug sink emit pointer not 4-byte aligned: 0x{emit:016x}",
            emit = emit_addr,
        );
    }
    if emit_addr <= 0x1000 {
        panic!(
            "debug sink emit pointer unexpectedly low: 0x{emit:016x}",
            emit = emit_addr,
        );
    }
    sel4_panicking::install_debug_sink(sink);
}

#[cfg(any(not(feature = "kernel"), all(feature = "kernel", sel4_config_printing)))]
/// No-op placeholder used when the kernel does not expose a debug sink attachment point.
#[inline(always)]
pub fn install_debug_sink() {}

#[cfg(not(feature = "kernel"))]
#[inline(always)]
pub fn debug_put_char(_ch: i32) {}

#[cfg(all(feature = "kernel", target_arch = "aarch64"))]
#[no_mangle]
/// Executes the `DebugPutChar` seL4 syscall to emit a byte on the debug console.
pub unsafe extern "C" fn seL4_DebugPutChar(byte: u8) {
    const SYS_DEBUG_PUT_CHAR: u64 = (!0u64).wrapping_sub(8); // -9
    unsafe {
        asm!(
            "svc #0",
            in("x0") u64::from(byte),
            lateout("x1") _,
            lateout("x2") _,
            lateout("x3") _,
            lateout("x4") _,
            lateout("x5") _,
            lateout("x6") _,
            in("x7") SYS_DEBUG_PUT_CHAR,
            options(nostack, preserves_flags),
        );
    }
}

#[cfg(all(feature = "kernel", target_arch = "aarch64", sel4_config_debug_build))]
#[inline(always)]
/// Requests the kernel to halt execution of the current thread via the debug syscall.
pub fn debug_halt() {
    const SYS_DEBUG_HALT: u64 = (!0u64).wrapping_sub(10); // -11

    unsafe {
        asm!(
            "svc #0",
            inout("x0") 0usize => _,
            lateout("x1") _,
            lateout("x2") _,
            lateout("x3") _,
            lateout("x4") _,
            lateout("x5") _,
            lateout("x6") _,
            in("x7") SYS_DEBUG_HALT,
            options(nostack, preserves_flags),
        );
    }
}

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", sel4_config_debug_build)))]
#[inline(always)]
/// Stub used when the kernel omits the debug halt syscall.
pub fn debug_halt() {}

#[cfg(all(feature = "kernel", target_arch = "aarch64", sel4_config_debug_build))]
#[no_mangle]
/// Executes the `DebugCapIdentify` seL4 syscall to reveal a capability's kernel tag.
pub unsafe extern "C" fn seL4_DebugCapIdentify(slot: seL4_CPtr) -> u32 {
    const SYS_DEBUG_CAP_IDENTIFY: i64 = -12;

    let mut badge = slot as usize;
    let mut info = 0usize;
    let mut mr0 = 0usize;
    let mut mr1 = 0usize;
    let mut mr2 = 0usize;
    let mut mr3 = 0usize;

    unsafe {
        asm!(
            "svc #0",
            inout("x0") badge,
            inout("x1") info,
            inout("x2") mr0,
            inout("x3") mr1,
            inout("x4") mr2,
            inout("x5") mr3,
            lateout("x6") _,
            in("x7") SYS_DEBUG_CAP_IDENTIFY as usize,
            options(nostack, preserves_flags),
        );
    }

    // Acknowledge the post-syscall register values to silence compiler warnings while retaining
    // the historical semantics of this debug helper.
    core::hint::black_box((info, mr0, mr1, mr2, mr3));

    badge as u32
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

#[cfg(not(feature = "kernel"))]
/// Returns zero because the function executes only when building for the host.
#[inline(always)]
pub fn debug_cap_identify(_slot: seL4_CPtr) -> seL4_Word {
    0
}

/// Safe projection of `seL4_CNode_Copy` for bootstrap modules.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_copy(
    bootinfo: &seL4_BootInfo,
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    rights: sel4_sys::seL4_CapRights,
) -> seL4_Error {
    debug_put_char(b'C' as i32);
    let depth_bits = bootinfo.init_cnode_depth();
    unsafe {
        seL4_CNode_Copy(
            dest_root, dest_index, depth_bits, src_root, src_index, depth_bits, rights,
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
        // SAFETY: Callers must ensure that the provided CNodes and depths originate from
        // kernel-supplied boot information. This wrapper centralises the unsafe invocation so
        // higher-level modules can remain within the crate-wide `#![deny(unsafe_code)]` policy.
        unsafe {
            seL4_CNode_Copy(
                dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights,
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

/// Safe projection of `seL4_CNode_Delete` for bootstrap modules.
#[cfg(feature = "kernel")]
#[inline(always)]
pub fn cnode_delete(root: seL4_CNode, index: seL4_CPtr, depth: u8) -> seL4_Error {
    debug_put_char(b'C' as i32);
    unsafe { seL4_CNode_Delete(root, index, depth) }
}

/// Safe projection of `seL4_CNode_Mint` for bootstrap modules.
#[cfg(feature = "kernel")]
#[deprecated(note = "use cspace_sys::*_invoc")]
#[inline(always)]
pub(crate) fn cnode_mint(
    bootinfo: &seL4_BootInfo,
    dest_root: seL4_CNode,
    dest_index: seL4_CPtr,
    src_root: seL4_CNode,
    src_index: seL4_CPtr,
    rights: sel4_sys::seL4_CapRights,
    badge: seL4_Word,
) -> seL4_Error {
    debug_put_char(b'C' as i32);
    let depth_bits = bootinfo.init_cnode_depth();
    unsafe {
        seL4_CNode_Mint(
            dest_root, dest_index, depth_bits, src_root, src_index, depth_bits, rights, badge,
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
        // SAFETY: Callers guarantee that the provided indices and depths stem from the
        // kernel-advertised CSpace topology, ensuring the kernel accepts the invocation.
        unsafe {
            seL4_CNode_Mint(
                dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights, badge,
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
                dest_root, dest_index, dest_depth, src_root, src_index, src_depth, rights, badge,
            )
        };
        ktry("cnode.mint", rc as i32)
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
    use sel4_sys::seL4_ObjectType::*;
    match t {
        x if x == seL4_UntypedObject as seL4_Word => "seL4_UntypedObject",
        x if x == seL4_TCBObject as seL4_Word => "seL4_TCBObject",
        x if x == seL4_EndpointObject as seL4_Word => "seL4_EndpointObject",
        x if x == seL4_NotificationObject as seL4_Word => "seL4_NotificationObject",
        x if x == seL4_CapTableObject as seL4_Word => "seL4_CapTableObject",
        x if x == seL4_ARM_Page as seL4_Word => "seL4_ARM_Page",
        x if x == seL4_ARM_LargePage as seL4_Word => "seL4_ARM_LargePage",
        x if x == seL4_ARM_PageTableObject as seL4_Word => "seL4_ARM_PageTableObject",
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
    use sel4_sys::seL4_ObjectType::*;
    match object_type {
        seL4_UntypedObject => "seL4_UntypedObject",
        seL4_TCBObject => "seL4_TCBObject",
        seL4_EndpointObject => "seL4_EndpointObject",
        seL4_NotificationObject => "seL4_NotificationObject",
        seL4_CapTableObject => "seL4_CapTableObject",
        seL4_ARM_Page => "seL4_ARM_Page",
        seL4_ARM_LargePage => "seL4_ARM_LargePage",
        seL4_ARM_PageTableObject => "seL4_ARM_PageTableObject",
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
    fn init_tcb_cap(&self) -> seL4_CPtr {
        seL4_CapInitThreadTCB
    }

    #[inline(always)]
    fn init_cnode_depth(&self) -> u8 {
        init_cnode_depth(self)
    }

    #[inline(always)]
    fn init_cnode_bits(&self) -> usize {
        self.initThreadCNodeSizeBits as usize
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
        match bootinfo_extra_slice(self) {
            Ok(slice) => slice,
            Err(err) => {
                log::error!("invalid bootinfo extra region: {err}");
                &[]
            }
        }
    }

    fn ipc_buffer_ptr(&self) -> Option<NonNull<sel4_sys::seL4_IPCBuffer>> {
        let ptr = self.ipcBuffer as *mut sel4_sys::seL4_IPCBuffer;
        NonNull::new(ptr)
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
    assert!(
        init_bits > 0,
        "BootInfo.initThreadCNodeSizeBits is 0 — capacity invalid"
    );
}

const PAGE_BITS: usize = 12;
const PAGE_TABLE_BITS: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_TABLE_ALIGN: usize = 1 << 21;
const PAGE_DIRECTORY_ALIGN: usize = 1 << 30;
const PAGE_UPPER_DIRECTORY_ALIGN: usize = 1 << 39;
const DEVICE_VADDR_BASE: usize = 0xA000_0000;
const DMA_VADDR_BASE: usize = 0xB000_0000;
const MAX_PAGE_TABLES: usize = 64;
const MAX_PAGE_DIRECTORIES: usize = 32;
const MAX_PAGE_UPPER_DIRECTORIES: usize = 8;
const DEVICE_VM_ATTRIBUTES: seL4_ARM_VMAttributes = seL4_ARM_VMAttributes(1 << 2);

/// Simple bump allocator for CSpace slots rooted at the initial thread's CNode.
pub struct SlotAllocator {
    cnode: seL4_CNode,
    start: seL4_CPtr,
    next: seL4_CPtr,
    end: seL4_CPtr,
    cnode_size_bits: seL4_Word,
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

    fn alloc(&mut self) -> Option<seL4_CPtr> {
        while self.next < self.end && is_boot_reserved_slot(self.next) {
            self.next += 1;
        }
        if self.next >= self.end {
            return None;
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
        Some(slot)
    }

    /// Attempt to allocate a slot without panicking when the window is exhausted.
    #[must_use]
    pub fn try_alloc(&mut self) -> Option<seL4_CPtr> {
        self.alloc()
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
    /// For the init thread's single-level CSpace this equals `initThreadCNodeSizeBits` because the
    /// kernel consumes the supplied root capability directly and addresses slots using the full
    /// radix width.
    #[inline(always)]
    pub fn depth(&self) -> seL4_Word {
        self.cnode_size_bits
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
#[inline(always)]
#[allow(non_upper_case_globals)]
pub fn is_boot_reserved_slot(slot: seL4_CPtr) -> bool {
    matches!(
        slot,
        seL4_CapNull
            | seL4_CapInitThreadTCB
            | seL4_CapInitThreadCNode
            | seL4_CapInitThreadVSpace
            | seL4_CapIRQControl
            | seL4_CapASIDControl
            | seL4_CapInitThreadASIDPool
            | seL4_CapIOPortControl
            | seL4_CapIOSpace
            | seL4_CapBootInfoFrame
            | seL4_CapInitThreadIPCBuffer
            | seL4_CapDomain
            | seL4_CapSMMUSIDControl
            | seL4_CapSMMUCBControl
            | seL4_CapInitThreadSC
            | seL4_CapSMC
    )
}

/// Handle to an untyped capability reserved from the bootinfo catalog.
pub struct ReservedUntyped {
    cap: seL4_Untyped,
    paddr: usize,
    size_bits: u8,
    index: usize,
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

    /// Returns the size of the reserved region in bits.
    #[must_use]
    pub fn size_bits(&self) -> u8 {
        self.size_bits
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

/// Index of bootinfo-provided untyped capabilities available to the root task.
pub struct UntypedCatalog<'a> {
    bootinfo: &'a seL4_BootInfo,
    entries: &'a [UntypedDesc],
    used: Vec<usize, MAX_BOOTINFO_UNTYPEDS>,
}

impl<'a> UntypedCatalog<'a> {
    /// Creates a catalog view over the untyped list exported by seL4.
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let count = bootinfo.untyped.end - bootinfo.untyped.start;
        let entries = &bootinfo.untypedList[..count as usize];
        Self {
            bootinfo,
            entries,
            used: Vec::new(),
        }
    }

    fn is_used(&self, index: usize) -> bool {
        self.used.iter().any(|&value| value == index)
    }

    fn reserve_index(&mut self, index: usize) -> Option<ReservedUntyped> {
        if self.is_used(index) {
            return None;
        }
        self.used.push(index).ok()?;
        let desc = &self.entries[index];
        Some(ReservedUntyped {
            cap: self.bootinfo.untyped.start + index as seL4_CPtr,
            paddr: desc.paddr as usize,
            size_bits: desc.sizeBits,
            index,
        })
    }

    /// Reserves an untyped covering the supplied device physical address range.
    pub fn reserve_device(&mut self, paddr: usize, size_bits: usize) -> Option<ReservedUntyped> {
        let end = paddr.saturating_add(1usize << size_bits);
        for (index, desc) in self.entries.iter().enumerate() {
            if desc.isDevice == 0 || self.is_used(index) {
                continue;
            }
            let base = desc.paddr as usize;
            let limit = base.saturating_add(1usize << desc.sizeBits);
            if base <= paddr && end <= limit {
                return self.reserve_index(index);
            }
        }
        None
    }

    /// Reserves the first RAM untyped meeting the requested size.
    pub fn reserve_ram(&mut self, min_size_bits: u8) -> Option<ReservedUntyped> {
        for (index, desc) in self.entries.iter().enumerate() {
            if desc.isDevice != 0 || desc.sizeBits < min_size_bits || self.is_used(index) {
                continue;
            }
            return self.reserve_index(index);
        }
        None
    }

    fn release_index(&mut self, index: usize) {
        if let Some(position) = self.used.iter().position(|&value| value == index) {
            let _ = self.used.swap_remove(position);
        }
    }

    /// Releases a previously reserved untyped so it may be reused.
    pub fn release(&mut self, reserved: &ReservedUntyped) {
        self.release_index(reserved.index);
    }

    /// Returns diagnostic statistics describing untyped catalogue utilisation.
    #[must_use]
    pub fn stats(&self) -> UntypedStats {
        let total = self.entries.len();
        let used = self.used.len();
        let device_total = self
            .entries
            .iter()
            .filter(|desc| desc.isDevice != 0)
            .count();
        let device_used = self
            .used
            .iter()
            .filter(|&&index| {
                self.entries
                    .get(index)
                    .map_or(false, |desc| desc.isDevice != 0)
            })
            .count();
        UntypedStats {
            total,
            used,
            device_total,
            device_used,
        }
    }

    /// Locates the device untyped covering the requested physical range, if available.
    #[must_use]
    pub fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        let end = paddr.saturating_add(1usize << size_bits);
        self.entries.iter().enumerate().find_map(|(index, desc)| {
            if desc.isDevice == 0 {
                return None;
            }
            let base = desc.paddr as usize;
            let limit = base.saturating_add(1usize << desc.sizeBits);
            if base <= paddr && end <= limit {
                Some(DeviceCoverage {
                    base,
                    limit,
                    size_bits: desc.sizeBits,
                    index,
                    used: self.is_used(index),
                })
            } else {
                None
            }
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
    /// Radix depth (in bits) used when submitting retype paths (equals `initThreadCNodeSizeBits`).
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
}

/// Detailed snapshot of the parameters used for a `seL4_Untyped_Retype` call.
///
/// The destination root **must** be the writable init thread CNode capability resident in slot
/// `seL4_CapInitThreadCNode`. Do not use allocator handles or read-only aliases. The init CSpace is
/// single-level, so the kernel consumes the supplied root capability directly. Root CNode policy for
/// this system: direct addressing with `node_depth = 0`, `node_index = 0`, and `dest_offset = dest_slot`.
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
    /// Root CNode policy for this system: `cnode_depth = 0` (direct root addressing).
    pub cnode_depth: seL4_Word,
    /// `nodeIndex` argument supplied to `seL4_Untyped_Retype` when selecting a sub-CNode below
    /// `cnode_root`. Root CNode policy for this system: `node_index = 0`.
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
    /// The guard depth did not match the init thread CSpace depth reported by bootinfo.
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
    /// The node index did not match the canonical init thread root traversal (slot-as-radix pointer).
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
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let root_cnode_bits = bootinfo.init_cnode_bits();
        assert!(
            root_cnode_bits > 0,
            "BootInfo.initThreadCNodeSizeBits is 0 — capacity invalid"
        );
        let capacity = 1usize
            .checked_shl(root_cnode_bits as u32)
            .unwrap_or_else(|| {
                panic!(
                    "initThreadCNodeSizeBits {} exceeds host word size",
                    root_cnode_bits
                )
            });
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
        let untyped = UntypedCatalog::new(bootinfo);
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
        }
    }

    /// Returns the bootinfo pointer passed to the root task.
    pub fn bootinfo(&self) -> &'a seL4_BootInfo {
        self.bootinfo
    }

    /// Returns a view over the init thread IPC buffer if it has been installed.
    pub fn ipc_buffer_view(&self) -> Option<IpcBufView> {
        self.ipcbuf_view
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

    /// Allocates a new CSpace slot, panicking if the root CNode is exhausted.
    pub fn allocate_slot(&mut self) -> seL4_CPtr {
        self.slots
            .alloc()
            .expect("cspace exhausted while allocating seL4 objects")
    }

    /// Maps a physical device frame into the root task's device window.
    pub fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_device(paddr, PAGE_BITS)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        #[cfg(target_arch = "aarch64")]
        let page_obj: seL4_Word = sel4_sys::seL4_ObjectType::seL4_ARM_Page as seL4_Word;
        #[cfg(target_arch = "aarch64")]
        let page_bits: seL4_Word = 12;

        #[cfg(not(target_arch = "aarch64"))]
        compile_error!("Wire correct page object type/size for non-AArch64 targets.");

        let dev_index = reserved.index();
        let dev_base_paddr = reserved.paddr();
        let dev_size_bits = reserved.size_bits();
        let dev_span = 1usize.checked_shl(dev_size_bits as u32).unwrap_or_else(|| {
            panic!(
                "device untyped size_bits {} exceeds host word size",
                dev_size_bits
            )
        });
        let dev_end_paddr = dev_base_paddr.saturating_add(dev_span);
        log::trace!(
            "device_untyped chosen: cap=0x{:x} idx={} covers=[0x{:08x}..0x{:08x}) size_bits={} target=0x{:08x}",
            reserved.cap(),
            dev_index,
            dev_base_paddr as u64,
            dev_end_paddr as u64,
            dev_size_bits,
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
        self.record_retype(trace, RetypeStatus::Ok);
        let vaddr = self
            .device_cursor
            .checked_add(PAGE_SIZE)
            .expect("device cursor overflow (address space exhausted)")
            - PAGE_SIZE;
        self.device_cursor += PAGE_SIZE;
        self.map_frame(frame_slot, vaddr, DEVICE_VM_ATTRIBUTES, false)?;
        Ok(DeviceFrame {
            cap: frame_slot,
            paddr,
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(vaddr))
                .expect("device mapping address must be non-null"),
        })
    }

    /// Maps the init thread's IPC buffer frame into the supplied virtual address.
    pub fn map_ipc_buffer(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        assert_ne!(vaddr, 0, "IPC buffer pointer must be non-null");
        let aligned = Self::align_down(vaddr, PAGE_SIZE);
        assert_eq!(
            aligned, vaddr,
            "IPC buffer pointer must be aligned to the page size"
        );

        self.ipcbuf_trace = true;
        let res = self.map_frame(
            seL4_CapInitThreadIPCBuffer,
            vaddr,
            seL4_ARM_Page_Default,
            true,
        );
        self.ipcbuf_trace = false;
        res
    }

    /// Binds the supplied IPC buffer frame to the provided TCB capability.
    pub fn bind_ipc_buffer(
        &mut self,
        tcb_cap: seL4_CPtr,
        buffer_vaddr: usize,
    ) -> Result<IpcBufView, seL4_Error> {
        debug_assert_ne!(buffer_vaddr, 0, "IPC buffer pointer must be non-null");
        if self.ipcbuf_trace {
            crate::bp!("ipcbuf.tcb.bind.begin");
        }

        let result = unsafe {
            sel4_sys::seL4_TCB_SetIPCBuffer(tcb_cap, buffer_vaddr, seL4_CapInitThreadIPCBuffer)
        };

        if result == seL4_NoError {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.tcb.bind.ok");
            }
            unsafe {
                sel4_sys::seL4_SetIPCBuffer(buffer_vaddr as *mut sel4_sys::seL4_IPCBuffer);
            }
            let view = unsafe { IpcBufView::new(buffer_vaddr as *const u8) };
            self.ipcbuf_view = Some(view);
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
            Err(result)
        }
    }

    /// Allocates a DMA-capable frame of RAM and maps it into the DMA window.
    pub fn alloc_dma_frame(&mut self) -> Result<RamFrame, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(PAGE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            PAGE_BITS as seL4_Word,
            RetypeKind::DmaPage {
                paddr: reserved.paddr(),
            },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);
        let vaddr = self
            .dma_cursor
            .checked_add(PAGE_SIZE)
            .expect("dma cursor overflow (address space exhausted)")
            - PAGE_SIZE;
        self.dma_cursor += PAGE_SIZE;
        self.map_frame(frame_slot, vaddr, seL4_ARM_Page_Uncached, false)?;
        Ok(RamFrame {
            cap: frame_slot,
            paddr: reserved.paddr(),
            ptr: NonNull::new(ptr::with_exposed_provenance_mut::<u8>(vaddr))
                .expect("DMA mapping address must be non-null"),
        })
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
            trace.object_type,
            seL4_ObjectType::seL4_ARM_Page as seL4_Word,
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
                trace.object_type,
                sel4_sys::seL4_ObjectType::seL4_ARM_Page as seL4_Word,
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
            unsafe {
                seL4_Untyped_Retype(
                    untyped_cap,
                    trace.object_type,
                    trace.object_size_bits,
                    trace.cnode_root,
                    trace.node_index,
                    trace.cnode_depth,
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
            unsafe {
                seL4_Untyped_Retype(
                    untyped_cap,
                    trace.object_type,
                    trace.object_size_bits,
                    trace.cnode_root,
                    trace.node_index,
                    trace.cnode_depth,
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
        let slot_limit = 1usize.checked_shl(init_bits as u32).unwrap_or_else(|| {
            panic!(
                "initThreadCNodeSizeBits {} exceeds host word size",
                init_bits
            )
        });
        let max_slots = slot_limit;

        let init_cnode = self.bootinfo.init_cnode_cap();
        let expected_depth: seL4_Word = 0;
        let expected_index: seL4_Word = 0;
        let expected_offset: seL4_Word = trace.dest_slot as seL4_Word;
        assert!(
            (trace.dest_slot as usize) < slot_limit,
            "Retype: dest_slot 0x{:x} out of range for init_bits={} (limit=0x{:x})",
            trace.dest_slot,
            init_bits,
            slot_limit,
        );

        assert_eq!(
            trace.cnode_root, init_cnode,
            "Retype: cnode_root 0x{:x} must equal init cnode 0x{:x}",
            trace.cnode_root, init_cnode
        );
        assert_eq!(
            trace.cnode_depth, expected_depth,
            "Retype: cnode_depth {} must equal {} when targeting the init CSpace root",
            trace.cnode_depth, expected_depth
        );

        let mut sanitised = trace;
        sanitised.cnode_root = init_cnode;
        sanitised.cnode_depth = expected_depth;

        let node_index = sanitised.node_index;
        assert!(
            (node_index as usize) < max_slots,
            "Retype: node_index 0x{:x} out of range (init_bits={}, max_slots={})",
            node_index,
            init_bits,
            max_slots
        );
        assert_eq!(
            node_index, expected_index,
            "Retype: node_index 0x{:x} must equal 0x{:x} when targeting the init CSpace root",
            node_index, expected_index
        );

        let dest_offset = sanitised.dest_offset;
        assert!(
            (dest_offset as usize) < max_slots,
            "Retype: dest_offset 0x{:x} out of range (init_bits={}, max_slots={})",
            dest_offset,
            init_bits,
            max_slots
        );
        assert_eq!(
            dest_offset, expected_offset,
            "Retype: dest_offset 0x{:x} must match expected offset 0x{:x} when targeting the init CSpace root",
            dest_offset,
            expected_offset
        );

        sanitised.node_index = expected_index;
        sanitised.dest_offset = expected_offset;

        (sanitised, init_bits)
    }

    fn map_frame(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        attr: sel4_sys::seL4_ARM_VMAttributes,
        strict: bool,
    ) -> Result<(), seL4_Error> {
        self.ensure_page_table(vaddr, strict)?;
        if self.ipcbuf_trace {
            crate::bp!("ipcbuf.page.retype.ok");
            crate::bp!("ipcbuf.page.map.begin");
        }
        let result = unsafe {
            seL4_ARM_Page_Map(
                frame_cap,
                seL4_CapInitThreadVSpace,
                vaddr,
                seL4_CapRights_ReadWrite,
                attr,
            )
        };

        if result == seL4_NoError {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.page.map.ok");
            }
            Ok(())
        } else if !strict && Self::mapping_already_present(result) {
            if self.ipcbuf_trace {
                crate::bp!("ipcbuf.page.map.ok");
            }
            Ok(())
        } else {
            let _ = crate::bootstrap::ktry("ipcbuf.page.map", result as i32);
            Err(result)
        }
    }

    fn align_down(value: usize, align: usize) -> usize {
        debug_assert!(align.is_power_of_two());
        value & !(align - 1)
    }

    #[inline(always)]
    fn mapping_already_present(err: seL4_Error) -> bool {
        err == sel4_sys::seL4_DeleteFirst || err == sel4_sys::seL4_IllegalOperation
    }

    fn ensure_page_table(&mut self, vaddr: usize, strict: bool) -> Result<(), seL4_Error> {
        self.ensure_page_directory(vaddr, strict)?;
        let pt_base = PageTableBookkeeper::<MAX_PAGE_TABLES>::base_for(vaddr);
        if self.page_tables.contains_base(pt_base) {
            return Ok(());
        }

        let reserved = self
            .untyped
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let pt_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pt_slot,
            seL4_ARM_PageTableObject,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageTable { vaddr: pt_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);
        if self.ipcbuf_trace {
            crate::bp!("ipcbuf.pt.retype.ok");
        }

        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pt_slot,
                seL4_CapInitThreadVSpace,
                pt_base,
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
            let _ = seL4_CNode_Delete(self.init_cnode_cap(), pt_slot, depth);
        }
        self.untyped.release(&reserved);

        if !strict && Self::mapping_already_present(map_res) {
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

    fn ensure_page_directory(&mut self, vaddr: usize, strict: bool) -> Result<(), seL4_Error> {
        let pd_base = PageDirectoryBookkeeper::<MAX_PAGE_DIRECTORIES>::base_for(vaddr);
        if self.page_directories.contains_base(pd_base) {
            return Ok(());
        }

        self.ensure_page_upper_directory(vaddr, strict)?;

        let reserved = self
            .untyped
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let pd_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pd_slot,
            seL4_ARM_PageTableObject,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageDirectory { vaddr: pd_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);

        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pd_slot,
                seL4_CapInitThreadVSpace,
                pd_base,
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
            let _ = seL4_CNode_Delete(self.init_cnode_cap(), pd_slot, depth);
        }
        self.untyped.release(&reserved);

        if !strict && Self::mapping_already_present(map_res) {
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

    fn ensure_page_upper_directory(
        &mut self,
        vaddr: usize,
        strict: bool,
    ) -> Result<(), seL4_Error> {
        let pud_base = PageUpperDirectoryBookkeeper::<MAX_PAGE_UPPER_DIRECTORIES>::base_for(vaddr);
        if self.page_upper_directories.contains_base(pud_base) {
            return Ok(());
        }

        let reserved = self
            .untyped
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let pud_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pud_slot,
            seL4_ARM_PageTableObject,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageUpperDirectory { vaddr: pud_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);

        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pud_slot,
                seL4_CapInitThreadVSpace,
                pud_base,
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
            let _ = seL4_CNode_Delete(self.init_cnode_cap(), pud_slot, depth);
        }
        self.untyped.release(&reserved);

        if !strict && Self::mapping_already_present(map_res) {
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
        // receive the new capability. Init-root retypes bypass guard-depth semantics entirely and
        // therefore rely on the canonical `(node_index = 0, node_depth = 0, dest_offset = slot)`
        // tuple so that the kernel addresses the slot directly within the root CNode.
        let cnode_root = self.bootinfo.init_cnode_cap();
        let node_index: seL4_Word = 0;
        let cnode_depth: seL4_Word = 0;
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

        if trace.cnode_root == init_cnode_cap {
            log::trace!(
                "Retype → root=0x{:x} (initCNode) index=0 depth=0 offset=0x{:x} (objtype={}({}), size_bits={}, untyped_paddr=0x{:08x})",
                trace.cnode_root,
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
        let expected_depth: seL4_Word = 0;
        let expected_index: seL4_Word = 0;
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(base, 0x0002_0000_0000);
    }

    #[test]
    fn header_bytes_span_entire_struct() {
        let bootinfo: seL4_BootInfo = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        let header = bootinfo.header_bytes();
        assert_eq!(header.len(), mem::size_of::<seL4_BootInfo>());
    }

    #[test]
    fn extra_bytes_returns_appended_region() {
        use core::mem::MaybeUninit;

        const EXTRA_WORDS: usize = 2;
        const EXTRA_BYTES: usize = EXTRA_WORDS * mem::size_of::<seL4_Word>();

        #[repr(C)]
        struct Fixture<const N: usize> {
            bootinfo: seL4_BootInfo,
            extra: [u8; N],
        }

        let mut fixture: Fixture<EXTRA_BYTES> = unsafe { MaybeUninit::zeroed().assume_init() };

        for (index, byte) in fixture.extra.iter_mut().enumerate() {
            *byte = index as u8;
        }

        fixture.bootinfo.extraLen = EXTRA_WORDS as seL4_Word;

        let extra = fixture.bootinfo.extra_bytes();
        assert_eq!(extra, &fixture.extra);
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
    fn contains_uses_alignment_when_tracking() {
        let mut keeper: PageTableBookkeeper<4> = PageTableBookkeeper::new();
        let base = PageTableBookkeeper::<4>::base_for(0xA000_0000);
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.contains(0xA000_0ABC));
        assert!(keeper.contains(0xA001_FFFF));
        assert!(!keeper.contains(0xA002_0000));
    }

    #[test]
    fn retype_trace_targets_root_cnode_slot() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let mut env = KernelEnv::new(bootinfo_ref);
        let reserved = ReservedUntyped {
            cap: 0x200,
            paddr: 0,
            size_bits: PAGE_BITS as u8,
            index: 0,
        };
        let slot: seL4_CPtr = 0x00c8;
        let trace = env.prepare_retype_trace(
            &reserved,
            slot,
            seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            PAGE_BITS as seL4_Word,
            RetypeKind::DevicePage { paddr: 0 },
        );
        assert_eq!(trace.cnode_root, bootinfo_ref.init_cnode_cap());
        let expected_index: seL4_Word = 0;
        let expected_depth: seL4_Word = 0;
        assert_eq!(trace.node_index, expected_index);
        assert_eq!(trace.cnode_depth, expected_depth);
        assert_eq!(trace.dest_offset, slot as seL4_Word);
        assert_eq!(trace.dest_slot, slot);
    }

    #[test]
    fn retype_sanitiser_uses_zero_depth() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let mut env = KernelEnv::new(bootinfo_ref);
        let dummy = ReservedUntyped {
            cap: 0x555,
            paddr: 0,
            size_bits: PAGE_TABLE_BITS as u8,
            index: 0,
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
        assert_eq!(sanitised.cnode_depth, seL4_WordBits as seL4_Word);
        assert_eq!(sanitised.node_index, 0);
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
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref);
        let init_root = bootinfo_ref.init_cnode_cap();

        let slot: seL4_CPtr = 0x00c8;
        let expected_depth: seL4_Word = 0;
        let canonical_index: seL4_Word = 0;
        let trace = RetypeTrace {
            untyped_cap: 0x200,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: init_root,
            dest_slot: slot,
            dest_offset: slot as seL4_Word,
            cnode_depth: expected_depth,
            node_index: canonical_index,
            object_type: seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DevicePage { paddr: 0 },
        };

        let (_, init_bits) = env.sanitise_retype_trace(trace);
        let max_slots = 1usize << init_bits;
        assert_eq!(init_bits, env.bootinfo().init_cnode_bits());
        assert!(slot as usize < max_slots);
    }

    #[test]
    fn retype_trace_is_root_slot() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref);
        let init_root = bootinfo_ref.init_cnode_cap();

        let slot: seL4_CPtr = 0x0097;
        let canonical_index: seL4_Word = 0;
        let expected_depth: seL4_Word = 0;
        let trace = RetypeTrace {
            untyped_cap: 0x100,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: init_root,
            dest_slot: slot,
            dest_offset: slot as seL4_Word,
            cnode_depth: expected_depth,
            node_index: canonical_index,
            object_type: seL4_ObjectType::seL4_ARM_Page as seL4_Word,
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
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref);
        let init_root = bootinfo_ref.init_cnode_cap();
        let expected_depth: seL4_Word = 0;
        let valid_trace = RetypeTrace {
            untyped_cap: 0x100,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: init_root,
            dest_slot: 0x1ff,
            dest_offset: 0x1ff,
            cnode_depth: expected_depth,
            node_index: 0,
            object_type: seL4_ObjectType::seL4_ARM_Page as seL4_Word,
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
}
