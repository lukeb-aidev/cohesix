// Author: Lukas Bower
//! BootInfo snapshotting utilities that defend against corruption during early bootstrap.
#![allow(unsafe_code)]

extern crate alloc;

use alloc::alloc::{alloc, Layout};
use core::mem;
use core::fmt;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

use heapless::String;
use sel4_sys::{seL4_BootInfo, seL4_Word};
use spin::Once;

use crate::bootstrap::log::force_uart_line;
use crate::sel4::{BootInfo, BootInfoError, BootInfoView};

const MAX_CANARY_LINE: usize = 192;

#[derive(Clone, Copy)]
pub struct BootInfoSnapshot {
    view: BootInfoView,
    backing: &'static [u8],
    bootinfo_addr: usize,
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

        let mut snapshot = Self {
            view,
            backing,
            bootinfo_addr: header as *const _ as usize,
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

        let checksum = snapshot.checksum();
        snapshot.checksum = checksum;
        snapshot
    }

    pub fn capture(view: &BootInfoView) -> Result<Self, BootInfoError> {
        let header_bytes = view.header_bytes();
        let extra_len = view.extra_bytes();

        let total_size = header_bytes
            .len()
            .checked_add(extra_len)
            .ok_or(BootInfoError::Overflow)?;
        let layout = Layout::from_size_align(total_size, mem::align_of::<seL4_BootInfo>())
            .map_err(|_| BootInfoError::Overflow)?;

        let backing_ptr = unsafe { alloc(layout) };
        if backing_ptr.is_null() {
            return Err(BootInfoError::Null);
        }

        unsafe {
            ptr::copy_nonoverlapping(header_bytes.as_ptr(), backing_ptr, header_bytes.len());
            if extra_len > 0 {
                ptr::copy_nonoverlapping(
                    view.extra().as_ptr(),
                    backing_ptr.add(header_bytes.len()),
                    extra_len,
                );
            }
        }

        let backing = unsafe { core::slice::from_raw_parts(backing_ptr, total_size) };
        let bootinfo_ptr = backing_ptr as *const seL4_BootInfo;
        let bootinfo_ref = unsafe { &*bootinfo_ptr };
        let snapshot_view = BootInfoView::new(bootinfo_ref)?;

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
}

impl fmt::Debug for BootInfoSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootInfoSnapshot")
            .field("bootinfo_addr", &format_args!("0x{:#x}", self.bootinfo_addr))
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
            .field("checksum", &self.checksum)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BootInfoCanaryError {
    Diverged {
        mark: &'static str,
        expected: BootInfoSnapshot,
        observed: BootInfoSnapshot,
    },
}

pub struct BootInfoState {
    view: BootInfoView,
    snapshot: BootInfoSnapshot,
    check_count: AtomicU64,
}

static BOOTINFO_STATE: Once<BootInfoState> = Once::new();

impl BootInfoState {
    pub fn init(bootinfo: &'static BootInfo) -> Result<&'static Self, BootInfoError> {
        let source_view = BootInfoView::new(bootinfo)?;
        let snapshot = BootInfoSnapshot::capture(&source_view)?;
        let snapshot_view = snapshot.view();

        Ok(BOOTINFO_STATE.call_once(|| Self {
            view: snapshot_view,
            snapshot,
            check_count: AtomicU64::new(0),
        }))
    }

    pub fn view(&self) -> BootInfoView {
        self.view
    }

    pub fn snapshot(&self) -> BootInfoSnapshot {
        self.snapshot
    }

    fn format_panic_line(
        &self,
        mark: &str,
        observed: &BootInfoSnapshot,
    ) -> String<MAX_CANARY_LINE> {
        let mut line = String::<MAX_CANARY_LINE>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!(
                "[bootinfo:canary] {mark} diverged: expected checksum=0x{exp:016x} observed=0x{obs:016x} checks={checks}",
                exp = self.snapshot.checksum,
                obs = observed.checksum,
                checks = self.check_count.load(Ordering::Relaxed)
            ),
        );
        line
    }

    pub fn verify_mark(&self, mark: &'static str) -> Result<(), BootInfoCanaryError> {
        let observed = BootInfoSnapshot::from_view(&self.view)
            .map_err(|_| BootInfoCanaryError::Diverged {
                mark,
                expected: self.snapshot,
                observed: self.snapshot,
            })?;
        self.check_count.fetch_add(1, Ordering::AcqRel);
        if self.snapshot.matches(&observed) {
            return Ok(());
        }

        let panic_line = self.format_panic_line(mark, &observed);
        force_uart_line(panic_line.as_str());
        log::error!("{}", panic_line.as_str());

        Err(BootInfoCanaryError::Diverged {
            mark,
            expected: self.snapshot,
            observed,
        })
    }
}
