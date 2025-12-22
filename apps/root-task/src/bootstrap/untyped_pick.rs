// Author: Lukas Bower

use core::{cmp::min, fmt::Write};

use crate::bootstrap::log::force_uart_line;
use crate::sel4::{device_window_range, BootInfo, BootInfoExt, PAGE_BITS, PAGE_TABLE_BITS};
use heapless::String;
use sel4_sys as sys;
use spin::Mutex;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DevicePtPoolConfig {
    pub ut_slot: sys::seL4_CPtr,
    pub paddr: usize,
    pub size_bits: u8,
    pub index: usize,
    pub total_bytes: usize,
}

static DEVICE_PT_POOL: Mutex<Option<DevicePtPoolConfig>> = Mutex::new(None);

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

fn device_pt_pool_index() -> Option<usize> {
    DEVICE_PT_POOL.lock().as_ref().map(|pool| pool.index)
}

fn register_device_pt_pool(cap: sys::seL4_CPtr, size_bits: u8, index: usize, paddr: usize) {
    let mut pool = DEVICE_PT_POOL.lock();
    if pool.is_some() {
        return;
    }

    debug_assert!(
        size_bits <= (usize::BITS.saturating_sub(1) as u8),
        "device pt pool size_bits exceeds host word width",
    );
    let total_bytes = 1usize
        .checked_shl(u32::from(size_bits))
        .expect("device pt pool size_bits overflowed host word width");
    let config = DevicePtPoolConfig {
        ut_slot: cap,
        paddr,
        size_bits,
        index,
        total_bytes,
    };

    let mut line = String::<192>::new();
    let _ = write!(
        line,
        "[retype:plan] reserved device PageTable pool ut=0x{cap:03x} bits={bits} paddr=0x{paddr:08x} capacity={bytes}B",
        bits = size_bits,
        paddr = paddr,
        bytes = total_bytes,
    );
    force_uart_line(line.as_str());

    *pool = Some(config);
}

pub fn device_pt_pool() -> Option<DevicePtPoolConfig> {
    DEVICE_PT_POOL.lock().as_ref().copied()
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
    /// Index of the untyped within the bootinfo list.
    pub index: usize,
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

pub fn ensure_device_pt_pool(bi: &'static BootInfo) {
    if device_pt_pool().is_some() {
        return;
    }

    // Each page table covers 512 entries of 4 KiB pages (2 MiB total on AArch64).
    // Reserve enough tables to cover the whole device window plus the upper levels.
    let window = device_window_range();
    let span = window.end.saturating_sub(window.start);
    let table_coverage = 1usize << (PAGE_BITS + 9);
    let l3_tables = (span + table_coverage - 1) / table_coverage;
    let upper_tables = if span == 0 { 0 } else { 2 };
    let required_tables = l3_tables.saturating_add(upper_tables);
    let required_bytes = required_tables.saturating_mul(1usize << PAGE_TABLE_BITS);

    let mut plan_line = String::<192>::new();
    let _ = write!(
        plan_line,
        "[retype:plan] device-pt window=[0x{start:08x}..0x{end:08x}) span={span} table_coverage={coverage} required_tables={tables} required_bytes={bytes}",
        start = window.start,
        end = window.end,
        span = span,
        coverage = table_coverage,
        tables = required_tables,
        bytes = required_bytes,
    );
    force_uart_line(plan_line.as_str());

    let total = (bi.untyped.end - bi.untyped.start) as usize;
    let entries = &bi.untypedList[..total];
    let ut_start = bi.untyped.start;

    let mut best_fit: Option<(usize, u8)> = None;
    let mut largest: Option<(usize, u8)> = None;

    for (offset, desc) in entries.iter().enumerate() {
        if desc.isDevice != 0 {
            continue;
        }
        let size_bits = desc.sizeBits as u8;
        if let Some((_, largest_bits)) = largest {
            if size_bits > largest_bits {
                largest = Some((offset, size_bits));
            }
        } else {
            largest = Some((offset, size_bits));
        }

        let capacity = 1usize
            .checked_shl(u32::from(size_bits))
            .unwrap_or(0);
        if capacity < required_bytes {
            continue;
        }

        match best_fit {
            None => best_fit = Some((offset, size_bits)),
            Some((_, best_bits)) => {
                if size_bits < best_bits {
                    best_fit = Some((offset, size_bits));
                }
            }
        }
    }

    let (index, size_bits, satisfied) = if let Some((index, size_bits)) = best_fit {
        (index, size_bits, true)
    } else if let Some((index, size_bits)) = largest {
        (index, size_bits, false)
    } else {
        force_uart_line("[bootstrap] device-pt pool unavailable: no RAM-backed untyped found");
        return;
    };

    let desc = &entries[index];
    let cap = ut_start + index as sys::seL4_CPtr;
    let available_bytes = 1usize
        .checked_shl(u32::from(size_bits))
        .unwrap_or(0);
    let available_tables = available_bytes / (1usize << PAGE_TABLE_BITS);
    let mut line = String::<192>::new();
    let _ = write!(
        line,
        "[bootstrap] device-pt pool selected: idx={idx} cap=0x{cap:03x} sizeBits={bits} paddr=0x{paddr:08x} tables={tables} required_tables={required} satisfied={ok}",
        idx = index,
        cap = cap,
        bits = desc.sizeBits,
        paddr = desc.paddr,
        tables = available_tables,
        required = required_tables,
        ok = satisfied as u8,
    );
    force_uart_line(line.as_str());

    register_device_pt_pool(cap, desc.sizeBits as u8, index, desc.paddr as usize);
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

    let reserve_bytes = page_table_bytes;
    let requested_pages = min(u128::from(u32::MAX), capacity_bytes / page_bytes) as u32;
    let available_pages = (capacity_bytes.saturating_sub(used_bytes + reserve_bytes) / page_bytes)
        .min(u128::from(requested_pages)) as u32;
    let used_after_pages = used_bytes
        .saturating_add(reserve_bytes)
        .saturating_add(page_bytes.saturating_mul(available_pages as u128));
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

    ensure_device_pt_pool(bi);
    let reserved_device_pool = device_pt_pool_index();

    for (offset, ut) in entries.iter().enumerate() {
        if Some(offset) == reserved_device_pool {
            continue;
        }

        if ut.isDevice == 0 && (ut.sizeBits as u8) >= min_bits {
            let cap = bi.untyped.start + offset as sys::seL4_CPtr;
            let selection = UntypedSelection {
                cap,
                index: offset,
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
        .find(|(index, ut)| ut.isDevice == 0 && Some(*index) != reserved_device_pool)
        .expect("bootinfo must provide at least one RAM-backed untyped capability");

    let cap = bi.untyped.start + offset as sys::seL4_CPtr;
    let selection = UntypedSelection {
        cap,
        index: offset,
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
        assert_eq!(plan.small_pages, 14);
        assert_eq!(plan.total, 15);
    }
}
