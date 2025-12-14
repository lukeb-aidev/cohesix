// Author: Lukas Bower
//! BootInfo snapshotting utilities that defend against corruption during early bootstrap.

use core::sync::atomic::{AtomicU64, Ordering};

use heapless::String;
use sel4_sys::seL4_Word;
use spin::Once;

use crate::bootstrap::log::force_uart_line;
use crate::sel4::{BootInfo, BootInfoError, BootInfoView};

const MAX_CANARY_LINE: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootInfoSnapshot {
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

    pub fn capture(view: &BootInfoView) -> Self {
        let (empty_start, empty_end) = view.init_cnode_empty_range();
        let extra_range = view.extra_range();
        let extra_start = extra_range.start;
        let extra_end = extra_range.end;
        let untyped_count = (view.header().untyped.end - view.header().untyped.start) as usize;
        let snapshot = Self {
            init_cnode_bits: view.init_cnode_bits(),
            empty_start,
            empty_end,
            untyped_start: view.header().untyped.start,
            untyped_end: view.header().untyped.end,
            untyped_count,
            ipc_buffer: view.header().ipcBuffer as usize,
            extra_start,
            extra_end,
            extra_len: view.extra_bytes(),
            checksum: 0,
        };

        let checksum = snapshot.checksum();
        Self {
            checksum,
            ..snapshot
        }
    }

    pub fn matches(&self, other: &Self) -> bool {
        self == other && self.checksum == other.checksum
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
        let view = BootInfoView::new(bootinfo)?;
        let snapshot = BootInfoSnapshot::capture(&view);

        Ok(BOOTINFO_STATE.call_once(|| Self {
            view,
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
                "[bootinfo:canary] {mark} diverged: expected checksum=0x{exp:016x} observed=0x{obs:016x} checks={}",
                self.snapshot.checksum,
                observed.checksum,
                self.check_count.load(Ordering::Relaxed)
            ),
        );
        line
    }

    pub fn verify_mark(&self, mark: &'static str) -> Result<(), BootInfoCanaryError> {
        let observed = BootInfoSnapshot::capture(&self.view);
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
