// Author: Lukas Bower

use core::{cmp::min, fmt::Write};

use crate::bootstrap::log::force_uart_line;
use crate::sel4::{BootInfo, BootInfoExt, PAGE_BITS, PAGE_TABLE_BITS};
use heapless::String;
use sel4_sys as sys;

/// Planned object counts derived from a RAM-backed untyped capability.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RetypePlan {
    /// Number of page table objects to mint from the untyped.
    pub page_tables: u32,
    /// Number of 4 KiB pages to derive from the untyped.
    pub small_pages: u32,
    /// Total objects scheduled for retype (page tables + pages).
    pub total: u32,
    /// Destination slot index at which the plan begins.
    pub dest_start: sys::seL4_CPtr,
}

impl RetypePlan {
    const fn new(page_tables: u32, small_pages: u32, dest_start: sys::seL4_CPtr) -> Self {
        let total = match page_tables.checked_add(small_pages) {
            Some(sum) => sum,
            None => u32::MAX,
        };
        Self {
            page_tables,
            small_pages,
            total,
            dest_start,
        }
    }
}

/// Selection outcome identifying the chosen untyped capability and its retype plan.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct UntypedSelection {
    /// Capability pointer referencing the selected untyped slot.
    pub cap: sys::seL4_CPtr,
    /// Size (in bits) reported by the kernel for the untyped.
    pub size_bits: u8,
    /// Bytes already consumed from this untyped by prior allocations.
    pub used_bytes: u128,
    /// Planned object counts derived from the untyped.
    pub plan: RetypePlan,
}

impl UntypedSelection {
    #[inline(always)]
    #[must_use]
    pub const fn capacity_bytes(&self) -> u128 {
        1u128 << self.size_bits
    }

    pub fn record_consumed(&mut self, obj_bits: u8) {
        self.used_bytes = self
            .used_bytes
            .saturating_add(1u128 << core::cmp::min(obj_bits, 127));
    }
}

fn log_plan_skip(
    cap: sys::seL4_CPtr,
    kind: &str,
    obj_bytes: u128,
    capacity_bytes: u128,
    used_bytes: u128,
) {
    let mut line = String::<192>::new();
    let _ = write!(
        line,
        "[retype:plan] skipping {kind} from ut=0x{cap:03x}: 1x{size}B would exceed {capacity}B capacity (used={used}B)",
        size = obj_bytes,
        capacity = capacity_bytes,
        used = used_bytes,
    );
    force_uart_line(line.as_str());
}

fn plan_for_untyped(cap: sys::seL4_CPtr, size_bits: u8, dest_start: sys::seL4_CPtr) -> RetypePlan {
    let capacity_bytes: u128 = 1u128 << size_bits;
    let mut used_bytes: u128 = 0;

    let page_table_bits = PAGE_TABLE_BITS as u8;
    let page_bits = PAGE_BITS as u8;
    let page_table_bytes = 1u128 << page_table_bits;
    let page_bytes = 1u128 << page_bits;

    let requested_page_tables: u32 = if size_bits >= page_table_bits { 1 } else { 0 };
    let available_tables =
        (capacity_bytes / page_table_bytes).min(requested_page_tables as u128) as u32;
    used_bytes =
        used_bytes.saturating_add(page_table_bytes.saturating_mul(available_tables as u128));
    if available_tables < requested_page_tables {
        log_plan_skip(
            cap,
            "PageTable",
            page_table_bytes,
            capacity_bytes,
            used_bytes,
        );
    }

    let requested_pages = min(u128::from(u32::MAX), capacity_bytes / page_bytes) as u32;
    let available_pages = (capacity_bytes.saturating_sub(used_bytes) / page_bytes)
        .min(u128::from(requested_pages)) as u32;
    let used_after_pages =
        used_bytes.saturating_add(page_bytes.saturating_mul(available_pages as u128));
    if available_pages < requested_pages {
        log_plan_skip(cap, "Page", page_bytes, capacity_bytes, used_after_pages);
    }

    RetypePlan::new(available_tables, available_pages, dest_start)
}

fn log_plan(selection: &UntypedSelection) {
    let mut line = String::<128>::new();
    let plan = selection.plan;
    let _ = write!(
        line,
        "[retype:plan] ut=0x{cap:03x} sz={bits} -> {pt}xPT + {pg}xPage (dest start=0x{start:04x})",
        cap = selection.cap,
        bits = selection.size_bits,
        pt = plan.page_tables,
        pg = plan.small_pages,
        start = plan.dest_start,
    );
    force_uart_line(line.as_str());
}

/// Returns the first RAM-backed untyped capability satisfying the requested size.
pub fn pick_untyped(bi: &'static BootInfo, min_bits: u8) -> UntypedSelection {
    let total = (bi.untyped.end - bi.untyped.start) as usize;
    let entries = &bi.untypedList[..total];
    let dest_start = bi.empty_first_slot() as sys::seL4_CPtr;

    for (offset, ut) in entries.iter().enumerate() {
        if ut.isDevice == 0 && (ut.sizeBits as u8) >= min_bits {
            let cap = bi.untyped.start + offset as sys::seL4_CPtr;
            let selection = UntypedSelection {
                cap,
                size_bits: ut.sizeBits as u8,
                used_bytes: 0,
                plan: plan_for_untyped(cap, ut.sizeBits as u8, dest_start),
            };
            log_plan(&selection);
            return selection;
        }
    }

    let (offset, ut) = entries
        .iter()
        .enumerate()
        .find(|(_, ut)| ut.isDevice == 0)
        .expect("bootinfo must provide at least one RAM-backed untyped capability");

    let cap = bi.untyped.start + offset as sys::seL4_CPtr;
    let selection = UntypedSelection {
        cap,
        size_bits: ut.sizeBits as u8,
        used_bytes: 0,
        plan: plan_for_untyped(cap, ut.sizeBits as u8, dest_start),
    };
    log_plan(&selection);
    selection
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retype_plan_total_saturates_on_overflow() {
        let plan = RetypePlan::new(u32::MAX, 42, 0x200);
        assert_eq!(plan.total, u32::MAX);
    }

    #[test]
    fn plan_for_untyped_clamps_small_pages() {
        let plan = plan_for_untyped(0x0200, 48, 0x0140);
        assert_eq!(plan.page_tables, 1);
        assert_eq!(plan.small_pages, u32::MAX);
        assert_eq!(plan.total, u32::MAX);
    }

    #[test]
    fn zero_size_yields_empty_plan() {
        let plan = plan_for_untyped(0x0100, 0, 0x0200);
        assert_eq!(plan.page_tables, 0);
        assert_eq!(plan.small_pages, 0);
        assert_eq!(plan.total, 0);
    }

    #[test]
    fn page_table_consumption_limits_pages() {
        let plan = plan_for_untyped(0x0100, 16, 0x010f);
        assert_eq!(plan.page_tables, 1);
        assert_eq!(plan.small_pages, 15);
        assert_eq!(plan.total, 16);
    }
}
