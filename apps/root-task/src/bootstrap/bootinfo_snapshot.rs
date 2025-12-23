// Author: Lukas Bower
//! BootInfo snapshotting utilities that defend against corruption during early bootstrap.
#![allow(unsafe_code)]

use core::arch::asm;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use heapless::String;
use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_Word};
use spin::Once;

use crate::bootinfo_layout::{post_canary_offset, POST_CANARY_BYTES};
use crate::bootstrap::log::{force_uart_line, uart_puthex_u64, uart_putnl, uart_puts};
use crate::sel4::{BootInfo, BootInfoError, BootInfoView, IPC_PAGE_BYTES};

#[cfg(feature = "bootinfo_guard_pages")]
use crate::sel4::{KernelEnv, ReserveVaddrError};

const MAX_CANARY_LINE: usize = 192;
const MAX_BOOTINFO_ALLOC: usize = 64 * 1024;
const HIGH_32_MASK: usize = 0xffff_ffff_0000_0000;
const BOOTINFO_CANARY_PRE: u64 = 0x0b0f_1ce5_ca4e_cafe;
const BOOTINFO_CANARY_POST: u64 = 0x9ddf_1ce5_f00d_beef;
#[allow(dead_code)]
const SNAPSHOT_GUARD_PAGES: bool = cfg!(feature = "bootinfo_guard_pages");
#[allow(dead_code)]
const SNAPSHOT_GUARD_POST_PAD: usize = 16;

const BOOT_HEAP_BYTES: usize = MAX_BOOTINFO_ALLOC;

#[repr(C, align(4096))]
struct BootinfoBacking {
    pre: u64,
    payload: [u8; MAX_BOOTINFO_ALLOC + POST_CANARY_BYTES],
}

static mut BOOTINFO_SNAPSHOT_BACKING: BootinfoBacking = BootinfoBacking {
    pre: BOOTINFO_CANARY_PRE,
    payload: [0u8; MAX_BOOTINFO_ALLOC + POST_CANARY_BYTES],
};

#[cfg(feature = "bootinfo_guard_pages")]
#[derive(Clone, Copy, Debug)]
struct GuardedBacking {
    frame_cap: seL4_CPtr,
    page_base: usize,
    backing_start: usize,
    backing_len: usize,
    post_canary_addr: usize,
    guard_range: core::ops::Range<usize>,
}

#[cfg(feature = "bootinfo_guard_pages")]
impl GuardedBacking {
    fn backing_slice(&self) -> &'static mut [u8] {
        unsafe {
            let ptr =
                core::ptr::with_exposed_provenance_mut::<u8>(self.backing_start) as *mut u8;
            core::slice::from_raw_parts_mut(ptr, self.backing_len)
        }
    }
}

#[cfg(feature = "bootinfo_guard_pages")]
static GUARDED_BACKING: Once<GuardedBacking> = Once::new();
#[cfg(feature = "bootinfo_guard_pages")]
static GUARD_ALLOC_WARNED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "bootinfo_guard_pages")]
static GUARD_ACTIVE_LOGGED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "bootinfo_guard_pages")]
static GUARD_READONLY: AtomicBool = AtomicBool::new(false);

fn log_layout_violation(reason: &str, size: usize, align: usize) {
    let mut line = String::<MAX_CANARY_LINE>::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "[bootinfo:snapshot] {reason}: size=0x{size:016x} align=0x{align:04x} heap_guard=0x{heap:016x}",
            heap = BOOT_HEAP_BYTES
        ),
    );
    force_uart_line(line.as_str());
}

#[inline(always)]
fn assert_low_vaddr(label: &str, value: usize) {
    if (value & HIGH_32_MASK) != 0 {
        panic!(
            "{} carries high address bits in low-vaddr build: 0x{value:016x}",
            label,
        );
    }
}

#[inline(always)]
fn post_canary_ptr_for(backing_ptr: *const u8, backing_len: usize) -> *const u64 {
    let offset = post_canary_offset(backing_len);
    unsafe { backing_ptr.add(offset) as *const u64 }
}

#[inline(always)]
fn set_post_canary(backing_ptr: *const u8, backing_len: usize) {
    let ptr = post_canary_ptr_for(backing_ptr, backing_len) as *mut u64;
    unsafe {
        core::ptr::write_volatile(ptr, BOOTINFO_CANARY_POST);
    }
}

#[cfg(feature = "bootinfo_guard_pages")]
fn guarded_backing() -> Option<&'static GuardedBacking> {
    GUARDED_BACKING.get()
}

#[cfg(feature = "bootinfo_guard_pages")]
fn guard_invariant_snapshot_in_page(snapshot: &BootInfoSnapshot, guarded: &GuardedBacking) {
    let page_start = guarded.page_base;
    let page_end = page_start.saturating_add(IPC_PAGE_BYTES);
    let post_addr = snapshot.post_canary_addr();
    if post_addr < page_start || post_addr >= page_end {
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "[bootinfo:guard] invariant failed: post_addr outside guard page base=0x{base:016x} post=0x{post:016x}",
                base = page_start,
                post = post_addr,
            ),
        );
        force_uart_line(line.as_str());
        panic!("bootinfo guard post-canary escaped guard page");
    }

    let backing_start = snapshot.backing().as_ptr() as usize;
    let backing_end = backing_start.saturating_add(snapshot.backing().len());
    if backing_start != guarded.backing_start || backing_start < page_start || backing_end > page_end
    {
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "[bootinfo:guard] invariant failed: snapshot backing outside guard page base=0x{base:016x} backing=0x{start:016x}..0x{end:016x}",
                base = page_start,
                start = backing_start,
                end = backing_end,
            ),
        );
        force_uart_line(line.as_str());
        panic!("bootinfo guard backing escaped guard page");
    }
}

#[cfg(feature = "bootinfo_guard_pages")]
pub fn install_guarded_backing(view: &BootInfoView, env: &mut KernelEnv) -> bool {
    let header_bytes = view.header_bytes();
    let extra_len = view.extra_bytes();
    let total_size = match header_bytes.len().checked_add(extra_len) {
        Some(size) => size,
        None => {
            if !GUARD_ALLOC_WARNED.swap(true, Ordering::AcqRel) {
                force_uart_line("[bootinfo:guard] allocation failed: snapshot size overflow");
            }
            return false;
        }
    };

    if total_size > IPC_PAGE_BYTES.saturating_sub(SNAPSHOT_GUARD_POST_PAD) {
        if !GUARD_ALLOC_WARNED.swap(true, Ordering::AcqRel) {
            force_uart_line("[bootinfo:guard] allocation failed: snapshot too large for guard page");
        }
        return false;
    }

    force_uart_line("[mark] bootinfo.guard.alloc.begin");
    let frame = match env.alloc_guard_frame() {
        Ok(frame) => frame,
        Err(err) => {
            if !GUARD_ALLOC_WARNED.swap(true, Ordering::AcqRel) {
                let mut line = String::<MAX_CANARY_LINE>::new();
                let _ = fmt::write(
                    &mut line,
                    format_args!("[bootinfo:guard] allocation failed: map err={err:?}"),
                );
                force_uart_line(line.as_str());
            }
            return false;
        }
    };

    let page_base = frame.ptr().as_ptr() as usize;
    let page_end = page_base.saturating_add(IPC_PAGE_BYTES);
    let backing_start = page_end
        .saturating_sub(SNAPSHOT_GUARD_POST_PAD)
        .saturating_sub(total_size);
    if backing_start < page_base {
        if !GUARD_ALLOC_WARNED.swap(true, Ordering::AcqRel) {
            force_uart_line("[bootinfo:guard] allocation failed: snapshot does not fit in page");
        }
        return false;
    }

    let guard_start = page_end;
    let guard_end = guard_start.saturating_add(IPC_PAGE_BYTES);
    let guard_range = guard_start..guard_end;
    if let Err(err) = env.try_reserve_vaddr_range(&guard_range, "bootinfo-guard") {
        if !GUARD_ALLOC_WARNED.swap(true, Ordering::AcqRel) {
            let mut line = String::<MAX_CANARY_LINE>::new();
            match err {
                ReserveVaddrError::Overlap {
                    conflict_start,
                    conflict_end,
                } => {
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "[bootinfo:guard] allocation failed: guard overlap=[0x{conflict_start:016x}..0x{conflict_end:016x})",
                        ),
                    );
                }
                ReserveVaddrError::Capacity => {
                    let _ = fmt::write(
                        &mut line,
                        format_args!("[bootinfo:guard] allocation failed: vaddr reserve full"),
                    );
                }
            }
            force_uart_line(line.as_str());
        }
        return false;
    }

    let post_canary_addr = backing_start.saturating_add(total_size);
    let backing = GuardedBacking {
        frame_cap: frame.cap(),
        page_base,
        backing_start,
        backing_len: total_size,
        post_canary_addr,
        guard_range,
    };
    let backing = GUARDED_BACKING.call_once(|| backing);
    GUARD_READONLY.store(false, Ordering::Release);

    let mut line = String::<MAX_CANARY_LINE>::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "[mark] bootinfo.guard.alloc.done mapped=[0x{base:016x}..0x{end:016x}) guard=[0x{guard_start:016x}..0x{guard_end:016x}) base=0x{backing_start:016x} post_addr=0x{post_addr:016x}",
            base = backing.page_base,
            end = backing.page_base.saturating_add(IPC_PAGE_BYTES),
            guard_start = backing.guard_range.start,
            guard_end = backing.guard_range.end,
            backing_start = backing.backing_start,
            post_addr = backing.post_canary_addr,
        ),
    );
    force_uart_line(line.as_str());
    true
}

#[cfg(feature = "bootinfo_guard_pages")]
pub fn guard_protect_readonly(env: &mut KernelEnv, snapshot: &BootInfoSnapshot) -> bool {
    if !SNAPSHOT_GUARD_PAGES {
        return false;
    }
    let Some(guarded) = guarded_backing() else {
        return false;
    };

    guard_invariant_snapshot_in_page(snapshot, guarded);

    force_uart_line("[mark] bootinfo.guard.protect.begin");
    let unmap_err = unsafe { sel4_sys::seL4_ARM_Page_Unmap(guarded.frame_cap) };
    if unmap_err != sel4_sys::seL4_NoError {
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "[bootinfo:guard] protect failed: unmap err={unmap_err} base=0x{base:016x}",
                base = guarded.page_base,
            ),
        );
        force_uart_line(line.as_str());
        panic!("bootinfo guard unmap failed");
    }

    let rights = sel4_sys::seL4_CapRights::new(0, 0, 1, 0);
    let attr = sel4_sys::seL4_ARM_VMAttributes(
        sel4_sys::seL4_ARM_Page_Default.0 | 0x04,
    );
    if let Err(err) = env.map_frame_with_rights(
        guarded.frame_cap,
        guarded.page_base,
        rights,
        attr,
        true,
    ) {
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "[bootinfo:guard] protect failed: remap err={err:?} base=0x{base:016x}",
                base = guarded.page_base,
            ),
        );
        force_uart_line(line.as_str());
        panic!("bootinfo guard remap failed");
    }

    GUARD_READONLY.store(true, Ordering::Release);
    let mut done_line = String::<MAX_CANARY_LINE>::new();
    let _ = fmt::write(
        &mut done_line,
        format_args!(
            "[mark] bootinfo.guard.protect.done base=0x{base:016x}",
            base = guarded.page_base,
        ),
    );
    force_uart_line(done_line.as_str());

    if !GUARD_ACTIVE_LOGGED.swap(true, Ordering::AcqRel) {
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "bootinfo.guard.active=1 base=0x{base:016x} post=0x{post:016x}",
                base = guarded.page_base,
                post = snapshot.post_canary_addr(),
            ),
        );
        force_uart_line(line.as_str());
    }

    true
}

#[derive(Clone, Copy)]
pub struct BootInfoSnapshot {
    view: BootInfoView,
    backing: &'static [u8],
    bootinfo_addr: usize,
    post_canary_addr: usize,
    pub init_cnode_bits: u8,
    pub empty_start: seL4_Word,
    pub empty_end: seL4_Word,
    pub untyped_start: seL4_Word,
    pub untyped_end: seL4_Word,
    pub untyped_count: usize,
    pub ipc_buffer: usize,
    pub extra_start: usize,
    pub extra_end: usize,
    pub extra_len: usize,
    checksum: u64,
}

impl BootInfoSnapshot {
    fn checksum(&self) -> u64 {
        let mut acc: u64 = 0x5a5a_a5a5_dead_beef;
        acc ^= u64::from(self.init_cnode_bits) << 8;
        acc = acc.wrapping_add(u64::from(self.empty_start ^ self.empty_end));
        acc ^= u64::from(self.untyped_start ^ self.untyped_end);
        acc = acc.wrapping_add(self.untyped_count as u64);
        acc ^= self.ipc_buffer as u64;
        acc = acc.wrapping_add((self.extra_start ^ self.extra_end) as u64);
        acc = acc.wrapping_add(self.extra_len as u64);
        acc.rotate_left(13)
    }

    fn from_parts(view: BootInfoView, backing: &'static [u8]) -> Self {
        let header = view.header();
        let (empty_start, empty_end) = view.init_cnode_empty_range();
        let extra_range = view.extra_range();
        let extra_len = view.extra_bytes();
        let untyped_count = (header.untyped.end - header.untyped.start) as usize;
        let post_canary_addr = (backing.as_ptr() as usize).saturating_add(backing.len());

        let mut snapshot = Self {
            view,
            backing,
            bootinfo_addr: header as *const _ as usize,
            post_canary_addr,
            init_cnode_bits: view.init_cnode_bits(),
            empty_start,
            empty_end,
            untyped_start: header.untyped.start,
            untyped_end: header.untyped.end,
            untyped_count,
            ipc_buffer: header.ipcBuffer as usize,
            extra_start: extra_range.start,
            extra_end: extra_range.end,
            extra_len,
            checksum: 0,
        };

        assert_low_vaddr("bootinfo header", snapshot.bootinfo_addr);
        assert_low_vaddr("ipc buffer", snapshot.ipc_buffer);
        assert_low_vaddr("bootinfo extra start", snapshot.extra_start);
        assert_low_vaddr("bootinfo extra end", snapshot.extra_end);

        let checksum = snapshot.checksum();
        snapshot.checksum = checksum;
        snapshot
    }

    pub fn capture(view: &BootInfoView) -> Result<Self, BootInfoError> {
        let header_bytes = view.header_bytes();
        let accessible_extra_len = view.extra().len();
        let total_size = header_bytes
            .len()
            .checked_add(accessible_extra_len)
            .ok_or(BootInfoError::Overflow)?;

        if total_size > BOOT_HEAP_BYTES {
            log_layout_violation("size exceeds heap guard", total_size, 0);
            return Err(BootInfoError::Overflow);
        }

        #[cfg(feature = "bootinfo_guard_pages")]
        if SNAPSHOT_GUARD_PAGES {
            if let Some(guarded) = guarded_backing() {
                if guarded.backing_len == total_size {
                    return Self::capture_with_backing(view, guarded.backing_slice());
                }
                force_uart_line("[bootinfo:guard] guarded backing size mismatch; refusing fallback");
                panic!("bootinfo guard backing size mismatch");
            }
        }

        let backing: &'static mut [u8] =
            unsafe { &mut BOOTINFO_SNAPSHOT_BACKING.payload[..total_size] };
        debug_assert!(
            total_size.saturating_add(POST_CANARY_BYTES)
                <= unsafe { BOOTINFO_SNAPSHOT_BACKING.payload.len() },
            "snapshot backing truncated before post-canary"
        );
        Self::capture_with_backing(view, backing)
    }

    fn capture_with_backing(
        view: &BootInfoView,
        backing: &'static mut [u8],
    ) -> Result<Self, BootInfoError> {
        let header_bytes = view.header_bytes();
        let accessible_extra_len = view.extra().len();

        unsafe {
            BOOTINFO_SNAPSHOT_BACKING.pre = BOOTINFO_CANARY_PRE;
        }

        backing[..header_bytes.len()].copy_from_slice(header_bytes);
        if accessible_extra_len > 0 {
            let extra_slice = view.extra();
            backing[header_bytes.len()..header_bytes.len() + accessible_extra_len]
                .copy_from_slice(extra_slice);
        }

        set_post_canary(backing.as_ptr(), backing.len());

        let bootinfo_ptr = backing.as_ptr() as *const seL4_BootInfo;
        let bootinfo_ref = unsafe { &*bootinfo_ptr };
        let snapshot_view = BootInfoView::from_snapshot_source(view, bootinfo_ref)?;

        Ok(Self::from_parts(snapshot_view, backing))
    }

    pub fn from_view(view: &BootInfoView) -> Result<Self, BootInfoError> {
        let header_bytes = view.header_bytes();
        let extra_len = view.extra_bytes();
        let total_size = header_bytes
            .len()
            .checked_add(extra_len)
            .ok_or(BootInfoError::Overflow)?;
        let backing = unsafe { core::slice::from_raw_parts(header_bytes.as_ptr(), total_size) };

        Ok(Self::from_parts(*view, backing))
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.init_cnode_bits == other.init_cnode_bits
            && self.empty_start == other.empty_start
            && self.empty_end == other.empty_end
            && self.untyped_start == other.untyped_start
            && self.untyped_end == other.untyped_end
            && self.untyped_count == other.untyped_count
            && self.ipc_buffer == other.ipc_buffer
            && self.extra_start == other.extra_start
            && self.extra_end == other.extra_end
            && self.extra_len == other.extra_len
            && self.bootinfo_addr == other.bootinfo_addr
            && self.backing.as_ptr() == other.backing.as_ptr()
            && self.backing.len() == other.backing.len()
            && self.post_canary_addr == other.post_canary_addr
            && self.checksum == other.checksum
    }

    #[must_use]
    pub fn view(&self) -> BootInfoView {
        self.view
    }

    #[must_use]
    pub fn bootinfo(&self) -> &'static BootInfo {
        self.view.header()
    }

    #[must_use]
    pub fn backing(&self) -> &'static [u8] {
        self.backing
    }

    #[must_use]
    pub fn post_canary_addr(&self) -> usize {
        self.post_canary_addr
    }

    fn uses_static_backing(&self) -> bool {
        let static_ptr = unsafe { BOOTINFO_SNAPSHOT_BACKING.payload.as_ptr() };
        self.backing.as_ptr() == static_ptr
    }
}

impl fmt::Debug for BootInfoSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootInfoSnapshot")
            .field(
                "bootinfo_addr",
                &format_args!("0x{:#x}", self.bootinfo_addr),
            )
            .field("init_cnode_bits", &self.init_cnode_bits)
            .field("empty_start", &self.empty_start)
            .field("empty_end", &self.empty_end)
            .field("untyped_start", &self.untyped_start)
            .field("untyped_end", &self.untyped_end)
            .field("untyped_count", &self.untyped_count)
            .field("ipc_buffer", &self.ipc_buffer)
            .field("extra_start", &self.extra_start)
            .field("extra_end", &self.extra_end)
            .field("extra_len", &self.extra_len)
            .field(
                "post_canary_addr",
                &format_args!("0x{:#x}", self.post_canary_addr),
            )
            .field("checksum", &self.checksum)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BootInfoSnapshotError {
    BootInfo(BootInfoError),
    OutOfBounds {
        start: usize,
        end: usize,
        limit: usize,
    },
}

impl fmt::Display for BootInfoSnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BootInfoSnapshotError::BootInfo(err) => fmt::Display::fmt(err, f),
            BootInfoSnapshotError::OutOfBounds { start, end, limit } => write!(
                f,
                "bootinfo snapshot out of bounds: [0x{start:016x}..0x{end:016x}) limit=0x{limit:016x}"
            ),
        }
    }
}

impl From<BootInfoError> for BootInfoSnapshotError {
    fn from(err: BootInfoError) -> Self {
        match err {
            BootInfoError::ExtraRange { start, end, limit } => {
                BootInfoSnapshotError::OutOfBounds { start, end, limit }
            }
            other => BootInfoSnapshotError::BootInfo(other),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BootInfoCanaryError {
    Diverged {
        mark: &'static str,
        expected: BootInfoSnapshot,
        observed: BootInfoSnapshot,
    },
    Snapshot {
        mark: &'static str,
        error: BootInfoSnapshotError,
    },
}

fn log_snapshot_failure(view: &BootInfoView, error: &BootInfoError) {
    let mut line = String::<MAX_CANARY_LINE>::new();
    let bootinfo_ptr = view.header() as *const _ as usize;
    let snapshot_ptr = {
        #[cfg(feature = "bootinfo_guard_pages")]
        if let Some(guarded) = guarded_backing() {
            guarded.backing_start
        } else {
            unsafe { BOOTINFO_SNAPSHOT_BACKING.payload.as_ptr() as usize }
        }
        #[cfg(not(feature = "bootinfo_guard_pages"))]
        unsafe {
            BOOTINFO_SNAPSHOT_BACKING.payload.as_ptr() as usize
        }
    };
    let header_len = view.header_bytes().len();
    let extra_len = view.extra().len();
    let total_size = header_len.saturating_add(extra_len);
    let pages = (total_size + IPC_PAGE_BYTES - 1) / IPC_PAGE_BYTES;
    let limit_base = bootinfo_ptr & !(IPC_PAGE_BYTES - 1);
    let limit_end = limit_base.saturating_add(pages.saturating_mul(IPC_PAGE_BYTES));
    let extra_range = view.extra_range();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "[bootinfo:snapshot:error] kind={error:?} src=0x{bootinfo_ptr:016x} dst=0x{snapshot_ptr:016x} total=0x{total_size:08x} pages={pages} limit=[0x{limit_base:016x}..0x{limit_end:016x}) extra=[0x{extra_start:016x}..0x{extra_end:016x}) len=0x{extra_len:08x}",
            extra_start = extra_range.start,
            extra_end = extra_range.end,
        ),
    );
    force_uart_line(line.as_str());
}

pub struct BootInfoState {
    view: BootInfoView,
    snapshot: BootInfoSnapshot,
    check_count: AtomicU64,
    snapshot_region: core::ops::Range<usize>,
}

static BOOTINFO_STATE: Once<BootInfoState> = Once::new();
static PROTECTED_RANGE_LOGGED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub(crate) fn protected_range() -> Option<(u64, u64)> {
    BootInfoState::get().map(|state| {
        let region = state.snapshot_region();
        (region.start as u64, region.end as u64)
    })
}

#[must_use]
pub(crate) fn protected_range_or_panic(tag: &'static str) -> (u64, u64) {
    protected_range().unwrap_or_else(|| {
        panic!("[bootinfo] protected_range unavailable: tag={tag}");
    })
}

#[must_use]
pub(crate) fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn read_sp() -> u64 {
    let sp: u64;
    unsafe {
        asm!("mov {sp}, sp", sp = out(reg) sp, options(nostack, preserves_flags));
    }
    sp
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn read_sp() -> u64 {
    0
}

pub(crate) fn assert_stack_not_in_protected(tag: &'static str) {
    let (start, end) = protected_range_or_panic(tag);
    let sp = read_sp();
    if sp >= start && sp < end {
        log::error!(
            target: "bootinfo",
            "[bootinfo] stack_overlaps_protected tag={tag} sp=0x{sp:016x} protected=0x{start:016x}..0x{end:016x}",
            sp = sp,
            start = start,
            end = end,
        );
        panic!(
            "[bootinfo] STACK_OVERLAP_BOOTINFO tag={tag} sp=0x{sp:016x} protected=0x{start:016x}..0x{end:016x}",
            sp = sp,
            start = start,
            end = end,
        );
    }
}

impl BootInfoState {
    #[must_use]
    pub fn get() -> Option<&'static Self> {
        BOOTINFO_STATE.get()
    }

    pub fn init(bootinfo: &'static BootInfo) -> Result<&'static Self, BootInfoError> {
        let source_view = BootInfoView::new(bootinfo)?;
        let snapshot = match BootInfoSnapshot::capture(&source_view) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log_snapshot_failure(&source_view, &err);
                return Err(err);
            }
        };
        let snapshot_view = snapshot.view();

        assert_low_vaddr("bootinfo pointer", bootinfo as *const _ as usize);

        let payload_start = snapshot.backing().as_ptr() as usize;
        let payload_end = payload_start.saturating_add(snapshot.backing().len());
        let canary_post = snapshot.post_canary_addr();
        let canary_end = canary_post.saturating_add(POST_CANARY_BYTES);
        if snapshot.uses_static_backing() {
            let canary_pre = unsafe { core::ptr::addr_of!(BOOTINFO_SNAPSHOT_BACKING.pre) as usize };
            debug_assert!(
                canary_post >= payload_end,
                "post-canary must trail snapshot payload"
            );
            debug_assert!(
                canary_end
                    <= payload_start
                        .saturating_add(unsafe { BOOTINFO_SNAPSHOT_BACKING.payload.len() }),
                "post-canary escaped backing payload"
            );
        }
        #[cfg(feature = "bootinfo_guard_pages")]
        if let Some(guarded) = guarded_backing() {
            guard_invariant_snapshot_in_page(&snapshot, guarded);
        }

        let state = BOOTINFO_STATE.call_once(|| Self {
            view: snapshot_view,
            snapshot,
            check_count: AtomicU64::new(0),
            snapshot_region: {
                let region_start = if snapshot.uses_static_backing() {
                    let canary_pre =
                        unsafe { core::ptr::addr_of!(BOOTINFO_SNAPSHOT_BACKING.pre) as usize };
                    canary_pre.min(payload_start)
                } else {
                    payload_start
                };
                region_start..canary_end.max(payload_end)
            },
        });
        if !PROTECTED_RANGE_LOGGED.swap(true, Ordering::AcqRel) {
            let region = state.snapshot_region();
            let post_addr = state.snapshot.post_canary_addr();
            let base = state.snapshot.backing().as_ptr() as usize;
            let len = state.snapshot.backing().len();
            let sp = read_sp();
            log::info!(
                target: "bootinfo",
                "[bootinfo] protected_range base=0x{base:016x} len=0x{len:08x} start=0x{start:016x} end=0x{end:016x} post_addr=0x{post_addr:016x} sp=0x{sp:016x}",
                base = base,
                len = len,
                start = region.start,
                end = region.end,
                post_addr = post_addr,
                sp = sp,
            );
        }
        assert_stack_not_in_protected("bootinfo.init");
        let _ = state.probe("[probe] snapshot.capture.complete");
        Ok(state)
    }

    pub fn view(&self) -> BootInfoView {
        self.view
    }

    pub fn snapshot(&self) -> BootInfoSnapshot {
        self.snapshot
    }

    pub fn snapshot_region(&self) -> core::ops::Range<usize> {
        self.snapshot_region.clone()
    }

    #[must_use]
    pub fn canary_values(&self) -> (u64, u64) {
        let post = unsafe {
            core::ptr::read_volatile(self.snapshot.post_canary_addr() as *const u64)
        };
        unsafe { (BOOTINFO_SNAPSHOT_BACKING.pre, post) }
    }

    #[must_use]
    pub fn canary_ok(&self) -> bool {
        let (pre, post) = self.canary_values();
        pre == BOOTINFO_CANARY_PRE && post == BOOTINFO_CANARY_POST
    }

    #[must_use]
    pub fn canary_state(&self) -> (u64, u64, u64, u64) {
        let (pre, post) = self.canary_values();
        (pre, post, BOOTINFO_CANARY_PRE, BOOTINFO_CANARY_POST)
    }

    pub fn probe(&self, mark: &'static str) -> Result<(), BootInfoError> {
        self.check_canaries("probe", mark);
        let (pre, post, exp_pre, exp_post) = self.canary_state();
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "[bootinfo:probe] {mark} pre=0x{pre:016x} post=0x{post:016x} expected_pre=0x{exp_pre:016x} expected_post=0x{exp_post:016x} post_addr=0x{addr:016x}",
                addr = self.snapshot.post_canary_addr(),
            ),
        );
        force_uart_line(line.as_str());

        Ok(())
    }

    fn check_canaries(&self, phase: &'static str, last_mark: &'static str) {
        let (pre, post) = self.canary_values();
        if pre == BOOTINFO_CANARY_PRE && post == BOOTINFO_CANARY_POST {
            return;
        }

        emit_corruption_report(
            phase,
            last_mark,
            pre,
            post,
            BOOTINFO_CANARY_PRE,
            BOOTINFO_CANARY_POST,
            self.snapshot.post_canary_addr(),
            self.snapshot_region.start,
            self.snapshot_region.end,
        );
        #[cfg(feature = "bootinfo_guard_pages")]
        if let Some(guarded) = guarded_backing() {
            let guarded_snapshot =
                (self.snapshot.backing().as_ptr() as usize) == guarded.backing_start;
            let ro = GUARD_READONLY.load(Ordering::Acquire);
            let mut line = String::<MAX_CANARY_LINE>::new();
            let _ = fmt::write(
                &mut line,
                format_args!(
                    "[bootinfo:guard] expected fault on write before canary; guarded={guarded} ro={ro}",
                    guarded = guarded_snapshot as u8,
                    ro = ro as u8,
                ),
            );
            force_uart_line(line.as_str());
        }
        panic!("BOOTINFO_SNAPSHOT_CORRUPTED");
    }

    pub fn verify(
        &self,
        phase: &'static str,
        mark: &'static str,
    ) -> Result<(), BootInfoCanaryError> {
        self.check_canaries(phase, mark);
        let observed = match BootInfoSnapshot::from_view(&self.view) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                let (pre, post, exp_pre, exp_post) = self.canary_state();
                emit_corruption_report(
                    phase,
                    mark,
                    pre,
                    post,
                    exp_pre,
                    exp_post,
                    self.snapshot.post_canary_addr(),
                    self.snapshot_region.start,
                    self.snapshot_region.end,
                );
                return Err(BootInfoCanaryError::Snapshot {
                    mark,
                    error: err.into(),
                });
            }
        };
        self.check_count.fetch_add(1, Ordering::AcqRel);
        if self.snapshot.matches(&observed) {
            return Ok(());
        }

        let (pre, post, exp_pre, exp_post) = self.canary_state();
        emit_corruption_report(
            phase,
            mark,
            pre,
            post,
            exp_pre,
            exp_post,
            self.snapshot.post_canary_addr(),
            self.snapshot_region.start,
            self.snapshot_region.end,
        );

        Err(BootInfoCanaryError::Diverged {
            mark,
            expected: self.snapshot,
            observed,
        })
    }
}

fn emit_corruption_report(
    phase: &str,
    last_mark: &str,
    pre: u64,
    post: u64,
    expected_pre: u64,
    expected_post: u64,
    post_addr: usize,
    region_start: usize,
    region_end: usize,
) {
    uart_puts(b"BOOTINFO_SNAPSHOT_CORRUPTED");
    uart_putnl();
    uart_puts(b"phase=");
    uart_puts(phase.as_bytes());
    uart_putnl();
    uart_puts(b"last_mark=");
    uart_puts(last_mark.as_bytes());
    uart_putnl();
    uart_puts(b"pre=");
    uart_puthex_u64(pre);
    uart_puts(b" post=");
    uart_puthex_u64(post);
    uart_puts(b" expected_pre=");
    uart_puthex_u64(expected_pre);
    uart_puts(b" expected_post=");
    uart_puthex_u64(expected_post);
    uart_putnl();
    uart_hex_usize(b"post_addr=", post_addr);
    let sp = current_sp();
    uart_hex_usize(b"sp=", sp);
    let sp_gt_post = sp > post_addr;
    uart_puts(b"sp_gt_post=");
    uart_puts(if sp_gt_post { b"1" } else { b"0" });
    uart_putnl();
    let delta = if sp_gt_post {
        sp.saturating_sub(post_addr)
    } else {
        post_addr.saturating_sub(sp)
    };
    uart_hex_usize(b"sp_post_delta=", delta);
    uart_hexdump_32(b"post_bytes=", post_addr, region_start, region_end);
}

#[cfg_attr(target_arch = "aarch64", allow(unsafe_code))]
fn current_sp() -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        let sp: usize;
        unsafe {
            core::arch::asm!(
                "mov {}, sp",
                out(reg) sp,
                options(nomem, nostack, preserves_flags)
            );
        }
        sp
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        0
    }
}

fn uart_hex_usize(label: &[u8], value: usize) {
    uart_puts(label);
    uart_puthex_u64(value as u64);
    uart_putnl();
}

#[allow(unsafe_code)]
fn uart_hexdump_32(label: &[u8], post_addr: usize, region_start: usize, region_end: usize) {
    uart_puts(label);
    let window_start = post_addr.saturating_sub(16);
    let window_end = post_addr.saturating_add(16);
    let start = core::cmp::max(window_start, region_start);
    let end = core::cmp::min(window_end, region_end);
    if end <= start {
        uart_putnl();
        return;
    }
    let mut first = true;
    for addr in start..end {
        if !first {
            uart_puts(b" ");
        }
        first = false;
        let byte = unsafe { core::ptr::read_volatile(addr as *const u8) };
        uart_puthex_u8(byte);
    }
    uart_putnl();
}

fn uart_puthex_u8(value: u8) {
    let hi = (value >> 4) & 0x0f;
    let lo = value & 0x0f;
    let hi_ascii = if hi < 10 { b'0' + hi } else { b'a' + (hi - 10) };
    let lo_ascii = if lo < 10 { b'0' + lo } else { b'a' + (lo - 10) };
    uart_puts(&[hi_ascii, lo_ascii]);
}

#[cfg(test)]
mod tests {
    use crate::bootinfo_layout::{post_canary_offset, POST_CANARY_BYTES};

    const BASE_ADDR: usize = 0x1000_0000;

    fn align_up(value: usize, align: usize) -> usize {
        (value + (align - 1)) & !(align - 1)
    }

    #[test]
    fn post_canary_tracks_payload_len() {
        let payload_len = 0x1800usize;
        let full_backing_len = payload_len + POST_CANARY_BYTES;
        let post_offset = post_canary_offset(payload_len);
        let post_addr = BASE_ADDR + post_offset;
        assert_eq!(post_addr, BASE_ADDR + full_backing_len - POST_CANARY_BYTES);

        let aligned_len = align_up(full_backing_len, 0x1000);
        assert_ne!(
            BASE_ADDR + aligned_len - POST_CANARY_BYTES,
            post_addr,
            "post-canary must not follow aligned padding"
        );
    }
}
