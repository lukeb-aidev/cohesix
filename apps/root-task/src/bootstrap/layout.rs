// Author: Lukas Bower
//! Early memory layout diagnostics to detect linker regressions before endpoint setup.
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::Write;

use heapless::String;

use crate::bootstrap::log::force_uart_line;

const REPORT_WIDTH: usize = 192;

extern "C" {
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_end: u8;
    static __data_end: u8;
    static __bss_start__: u8;
    static __bss_end__: u8;
    static __heap_start: u8;
    static __heap_end: u8;
    static __stack_bottom: u8;
    static __stack_top: u8;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct LayoutSnapshot {
    text_start: usize,
    text_end: usize,
    rodata_end: usize,
    data_end: usize,
    bss_start: usize,
    bss_end: usize,
    heap_start: usize,
    heap_end: usize,
    stack_bottom: usize,
    stack_top: usize,
}

impl LayoutSnapshot {
    const fn new(
        text_start: usize,
        text_end: usize,
        rodata_end: usize,
        data_end: usize,
        bss_start: usize,
        bss_end: usize,
        heap_start: usize,
        heap_end: usize,
        stack_bottom: usize,
        stack_top: usize,
    ) -> Self {
        Self {
            text_start,
            text_end,
            rodata_end,
            data_end,
            bss_start,
            bss_end,
            heap_start,
            heap_end,
            stack_bottom,
            stack_top,
        }
    }

    fn from_linker() -> Self {
        Self::new(
            core::ptr::addr_of!(__text_start) as usize,
            core::ptr::addr_of!(__text_end) as usize,
            core::ptr::addr_of!(__rodata_end) as usize,
            core::ptr::addr_of!(__data_end) as usize,
            core::ptr::addr_of!(__bss_start__) as usize,
            core::ptr::addr_of!(__bss_end__) as usize,
            core::ptr::addr_of!(__heap_start) as usize,
            core::ptr::addr_of!(__heap_end) as usize,
            core::ptr::addr_of!(__stack_bottom) as usize,
            core::ptr::addr_of!(__stack_top) as usize,
        )
    }

    fn validate(&self) -> Result<(), LayoutError> {
        if self.heap_start < self.bss_end {
            return Err(LayoutError::HeapBeforeBssEnd(self.heap_start, self.bss_end));
        }

        if self.heap_end > self.stack_bottom {
            return Err(LayoutError::HeapOverlapsStack(
                self.heap_end,
                self.stack_bottom,
            ));
        }

        if self.stack_top <= self.stack_bottom {
            return Err(LayoutError::StackOrder(self.stack_bottom, self.stack_top));
        }

        Ok(())
    }

    fn fmt_report(&self) -> String<REPORT_WIDTH> {
        let mut line = String::<REPORT_WIDTH>::new();
        let _ = write!(
            line,
            "[boot:layout] text=[0x{txt_start:08x}..0x{txt_end:08x}) rodata_end=0x{ro_end:08x} data_end=0x{data_end:08x} bss=[0x{bss_start:08x}..0x{bss_end:08x}) heap=[0x{heap_start:08x}..0x{heap_end:08x}) stack=[0x{stack_bottom:08x}..0x{stack_top:08x})",
            txt_start = self.text_start,
            txt_end = self.text_end,
            ro_end = self.rodata_end,
            data_end = self.data_end,
            bss_start = self.bss_start,
            bss_end = self.bss_end,
            heap_start = self.heap_start,
            heap_end = self.heap_end,
            stack_bottom = self.stack_bottom,
            stack_top = self.stack_top,
        );
        line
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutError {
    HeapBeforeBssEnd(usize, usize),
    HeapOverlapsStack(usize, usize),
    StackOrder(usize, usize),
}

impl LayoutError {
    fn render(&self) -> String<REPORT_WIDTH> {
        let mut line = String::<REPORT_WIDTH>::new();
        match self {
            Self::HeapBeforeBssEnd(heap_start, bss_end) => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: heap overlaps bss (heap_start=0x{heap_start:08x} bss_end=0x{bss_end:08x})"
                );
            }
            Self::HeapOverlapsStack(heap_end, stack_bottom) => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: heap overlaps stack (heap_end=0x{heap_end:08x} stack_bottom=0x{stack_bottom:08x})"
                );
            }
            Self::StackOrder(stack_bottom, stack_top) => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: stack ordering invalid (stack_bottom=0x{stack_bottom:08x} stack_top=0x{stack_top:08x})"
                );
            }
        }
        line
    }
}

/// Emit a single-line layout report and halt the system if the linker-provided
/// segments overlap in an unexpected way. Safe to call before capability setup
/// or DTB parsing.
pub fn dump_and_sanity_check() {
    let layout = LayoutSnapshot::from_linker();
    let report = layout.fmt_report();

    force_uart_line(report.as_str());
    log::info!("{}", report.as_str());

    if let Err(err) = layout.validate() {
        let error_line = err.render();
        force_uart_line(error_line.as_str());
        log::error!("{}", error_line.as_str());
        crate::sel4::debug_halt();
        loop {
            core::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LayoutError, LayoutSnapshot};

    #[test]
    fn layout_validation_flags_overlap() {
        let layout = LayoutSnapshot::new(0, 1, 1, 2, 2, 3, 1, 4, 4, 5);
        assert_eq!(layout.validate(), Err(LayoutError::HeapBeforeBssEnd(1, 3)));

        let layout = LayoutSnapshot::new(0, 1, 1, 2, 2, 3, 3, 5, 4, 5);
        assert_eq!(layout.validate(), Err(LayoutError::HeapOverlapsStack(5, 4)));

        let layout = LayoutSnapshot::new(0, 1, 1, 2, 2, 3, 3, 4, 6, 6);
        assert_eq!(layout.validate(), Err(LayoutError::StackOrder(6, 6)));
    }

    #[test]
    fn layout_validation_accepts_ordered_ranges() {
        let layout = LayoutSnapshot::new(0, 1, 2, 3, 4, 5, 6, 7, 8, 9);
        assert_eq!(layout.validate(), Ok(()));
    }
}
