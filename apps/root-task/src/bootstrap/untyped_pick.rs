// Author: Lukas Bower

use core::{cmp::min, fmt::Write};

use crate::bootstrap::log::force_uart_line;
use crate::sel4::{BootInfo, BootInfoExt};
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
    /// Planned object counts derived from the untyped.
    pub plan: RetypePlan,
}

fn plan_for_untyped(size_bits: u8, dest_start: sys::seL4_CPtr) -> RetypePlan {
    let mut remaining_bytes: u64 = 1u64 << size_bits;
    let page_table_bits = sel4_sys::seL4_PageTableBits as u8;
    let page_bits = sel4_sys::seL4_PageBits as u8;

    let mut page_tables = 0u32;
    if size_bits >= page_table_bits {
        page_tables = 1;
        let pt_bytes = 1u64 << page_table_bits;
        remaining_bytes = remaining_bytes.saturating_sub(pt_bytes);
    }

    let page_bytes = 1u64 << page_bits;
    let small_pages = if remaining_bytes >= page_bytes {
        let raw = remaining_bytes / page_bytes;
        min(raw, u32::MAX as u64) as u32
    } else {
        0
    };

    let plan = RetypePlan::new(page_tables, small_pages, dest_start);
    if plan.total == 0 {
        RetypePlan::new(0, 1, dest_start)
    } else {
        plan
    }
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
                plan: plan_for_untyped(ut.sizeBits as u8, dest_start),
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
        plan: plan_for_untyped(ut.sizeBits as u8, dest_start),
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
        let plan = plan_for_untyped(48, 0x0140);
        assert_eq!(plan.page_tables, 1);
        assert_eq!(plan.small_pages, u32::MAX);
        assert_eq!(plan.total, u32::MAX);
    }

    #[test]
    fn zero_size_promotes_single_page_plan() {
        let plan = plan_for_untyped(0, 0x0200);
        assert_eq!(plan.page_tables, 0);
        assert_eq!(plan.small_pages, 1);
        assert_eq!(plan.total, 1);
    }
}
