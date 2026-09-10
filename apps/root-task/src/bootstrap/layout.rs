// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Early linker layout diagnostics and reporting during root-task bootstrap.
// Author: Lukas Bower
//! Early memory layout diagnostics to detect linker regressions before endpoint setup.
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::Write;

use heapless::String;
use sel4_sys;

use crate::bootstrap::log::force_uart_line;

const STACK_ALIGNMENT: usize = 16;
// Keep this independent policy guard aligned with `sel4.ld`: the retained
// The emitted bootstrap/GENET chain alone exceeds 512 KiB before deeper calls.
// The 1 MiB policy includes headroom for retained supervisor/service frames.
const EXPECTED_STACK_SIZE: usize = 0x0010_0000;

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

#[cfg(any(test, all(feature = "release-pi4", feature = "bootstrap-trace")))]
fn root_text_word_checksum(mut hash: u32, word: u32) -> u32 {
    for byte in word.to_le_bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(any(test, all(feature = "release-pi4", feature = "bootstrap-trace")))]
mod root_text_retention {
    use core::fmt::Write;
    use core::sync::atomic::{AtomicBool, Ordering};

    use heapless::String;
    use spin::Mutex;

    const CUTS: [&str; 3] = ["root-entry", "ipc-installed", "fault-receivers-active"];
    const COMPLETE_MASK: u8 = 0b111;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) struct Sample {
        pub(super) start: usize,
        pub(super) hash: u32,
        pub(super) word_34: u32,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Publication {
        captured: u8,
        published: u8,
        capture_failed: bool,
    }

    impl Publication {
        fn complete(self) -> bool {
            self.captured == COMPLETE_MASK
                && self.published == COMPLETE_MASK
                && !self.capture_failed
        }
    }

    struct Samples {
        slots: [Option<Sample>; 3],
        publication_attempted: bool,
        capture_failed: bool,
    }

    impl Samples {
        const fn new() -> Self {
            Self {
                slots: [None; 3],
                publication_attempted: false,
                capture_failed: false,
            }
        }

        fn capture(&mut self, slot: usize, sample: Sample) -> bool {
            if self.publication_attempted {
                self.capture_failed = true;
                return false;
            }
            let Some(destination) = self.slots.get_mut(slot) else {
                self.capture_failed = true;
                return false;
            };
            if destination.is_some() {
                self.capture_failed = true;
                return false;
            }
            *destination = Some(sample);
            true
        }

        fn publish_once(
            &mut self,
            mut publish: impl FnMut(&str, Sample) -> bool,
        ) -> Option<Publication> {
            if self.publication_attempted {
                return None;
            }
            self.publication_attempted = true;
            let mut result = Publication {
                captured: 0,
                published: 0,
                capture_failed: self.capture_failed,
            };
            for (slot, sample) in self.slots.iter().enumerate() {
                if let Some(sample) = sample {
                    result.captured |= 1 << slot;
                    if publish(CUTS[slot], *sample) {
                        result.published |= 1 << slot;
                    }
                }
            }
            Some(result)
        }
    }

    // Only root bootstrap writes these fixed samples. try_lock never waits on
    // another TCB; original values remain private and are never overwritten.
    static EARLY_SAMPLES: Mutex<Samples> = Mutex::new(Samples::new());
    static CAPTURE_CONTENDED: AtomicBool = AtomicBool::new(false);

    pub(super) fn render(cut: &str, sample: Sample) -> Result<String<224>, core::fmt::Error> {
        let mut line = String::new();
        write!(
            line,
            "[diag root-text/v1] cut={cut} start=0x{:x} bytes=4092 fnv1a32=0x{:08x} word34=0x{:08x}",
            sample.start, sample.hash, sample.word_34,
        )?;
        Ok(line)
    }

    pub(super) fn capture_early(cut: &str, sample: Sample) -> bool {
        let Some(slot) = CUTS.iter().position(|candidate| *candidate == cut) else {
            return false;
        };
        if let Some(mut retained) = EARLY_SAMPLES.try_lock() {
            retained.capture(slot, sample);
        } else {
            CAPTURE_CONTENDED.store(true, Ordering::Release);
        }
        true
    }

    pub(super) fn publish() {
        let result = if let Some(mut retained) = EARLY_SAMPLES.try_lock() {
            retained.publish_once(|cut, sample| {
                let Ok(line) = render(cut, sample) else {
                    return false;
                };
                // Each rendered record fits the existing 256-byte log line.
                // This nonblocking sink acknowledges actual retention; there
                // is no UART operation or fresh text-page read while locked.
                crate::log_buffer::try_append_boot_audit_line(line.as_str())
            })
        } else {
            crate::bootstrap::log::retain_bootstrap_audit_line(
                "[diag root-text-retention/v1] state=failed reason=publication-lock-contended",
            );
            return;
        };
        let Some(mut result) = result else {
            return;
        };
        result.capture_failed |= CAPTURE_CONTENDED.load(Ordering::Acquire);
        let mut line = String::<192>::new();
        let formatted = write!(
            line,
            "[diag root-text-retention/v1] state={} source=capture-time publish=pre-pcie captured=0x{:x} published=0x{:x} capture_failed={}",
            if result.complete() { "complete" } else { "failed" },
            result.captured,
            result.published,
            u8::from(result.capture_failed),
        );
        if formatted.is_err() || !crate::log_buffer::try_append_boot_audit_line(line.as_str()) {
            crate::bootstrap::log::force_uart_line(
                "[diag root-text-retention/v1] state=failed reason=summary-retention-unavailable",
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{render, Sample, Samples};

        fn sample(value: u32) -> Sample {
            Sample {
                start: 4,
                hash: value,
                word_34: value + 1,
            }
        }

        #[test]
        fn original_cut_values_survive_until_one_ordered_publication() {
            let mut samples = Samples::new();
            assert!(samples.capture(1, sample(20)));
            assert!(samples.capture(0, sample(10)));
            assert!(samples.capture(2, sample(30)));
            let mut emitted = std::vec::Vec::new();
            let result = samples.publish_once(|cut, sample| {
                emitted.push((cut.to_owned(), sample));
                true
            });
            assert!(result.expect("first publication exists").complete());
            assert_eq!(
                emitted,
                [
                    ("root-entry".to_owned(), sample(10)),
                    ("ipc-installed".to_owned(), sample(20)),
                    ("fault-receivers-active".to_owned(), sample(30)),
                ]
            );
            assert_eq!(
                samples.publish_once(|_, _| panic!("no second publication")),
                None
            );
            assert!(!samples.capture(0, sample(40)));
            assert_eq!(samples.slots[0], Some(sample(10)));
        }

        #[test]
        fn duplicate_and_out_of_range_capture_never_replace_original_sample() {
            let mut samples = Samples::new();
            assert!(samples.capture(0, sample(10)));
            assert!(!samples.capture(0, sample(20)));
            assert!(!samples.capture(3, sample(30)));
            assert_eq!(samples.slots, [Some(sample(10)), None, None]);
            let result = samples
                .publish_once(|_, _| true)
                .expect("first publication");
            assert_eq!(result.captured, 0b001);
            assert_eq!(result.published, 0b001);
            assert!(result.capture_failed);
            assert!(!result.complete());
        }

        #[test]
        fn missing_and_unaccepted_records_cannot_be_reported_complete() {
            let mut samples = Samples::new();
            assert!(samples.capture(0, sample(10)));
            assert!(samples.capture(2, sample(30)));
            let result = samples
                .publish_once(|cut, _| cut == "root-entry")
                .expect("first publication");
            assert_eq!(result.captured, 0b101);
            assert_eq!(result.published, 0b001);
            assert!(!result.capture_failed);
            assert!(!result.complete());
            assert_eq!(samples.slots[2], Some(sample(30)));
            assert_eq!(
                samples.publish_once(|_, _| panic!("no second publication")),
                None
            );
        }

        #[test]
        fn capture_time_record_format_preserves_values_and_existing_line_bound() {
            assert_eq!(
                render("root-entry", sample(0x1234)).expect("bounded format").as_str(),
                "[diag root-text/v1] cut=root-entry start=0x4 bytes=4092 fnv1a32=0x00001234 word34=0x00001235"
            );
            let longest = render(
                "fault-receivers-active",
                Sample {
                    start: usize::MAX,
                    hash: u32::MAX,
                    word_34: u32::MAX,
                },
            )
            .expect("known longest early cut fits");
            assert!(longest.len() <= 224);
            assert!(render(&"x".repeat(224), sample(1)).is_err());
        }
    }
}

/// Publish the three original early samples after bulk child admission, before
/// either Pi network lane begins PCIe and constructor diagnostics. Publication
/// performs no text reads; missing/rejected samples remain evidence failures.
#[cfg(all(
    feature = "release-pi4",
    feature = "bootstrap-trace",
    target_os = "none"
))]
pub(crate) fn publish_early_root_text_samples() {
    root_text_retention::publish();
}

/// Observe the first mapped text page at fixed bootstrap boundaries. The
/// loaded ELF supplies the independent expected checksum and instruction word;
/// this sample neither changes mappings nor repairs or resumes a fault.
#[cfg(all(
    feature = "release-pi4",
    feature = "bootstrap-trace",
    target_os = "none"
))]
pub(crate) fn trace_root_text(cut: &str) {
    let text_start = core::ptr::addr_of!(__text_start) as usize;
    let text_end = core::ptr::addr_of!(__text_end) as usize;
    let Some(end) = text_start.checked_add(4096).filter(|end| *end <= text_end) else {
        force_uart_line("[diag root-text/v1] state=invalid-linker-span");
        return;
    };
    if text_start & 4095 != 0 {
        force_uart_line("[diag root-text/v1] state=unaligned-linker-span");
        return;
    }
    // Exclude the entry word at virtual zero: never form or dereference a null
    // Rust pointer even though the selected kernel maps the first text page.
    let start = text_start + 4;
    let mut hash = 0x811c_9dc5u32;
    let mut word_34 = 0u32;
    for address in (start..end).step_by(4) {
        // SAFETY: The linker-aligned interval is wholly within the first
        // kernel-mapped root text page, remains mapped throughout bootstrap,
        // and each non-null address is u32-aligned. Only volatile reads occur;
        // no reference or write alias is created over executable storage.
        let word = unsafe { core::ptr::read_volatile(address as *const u32) };
        hash = root_text_word_checksum(hash, word);
        if address - text_start == 0x34 {
            word_34 = word;
        }
    }
    let sample = root_text_retention::Sample {
        start,
        hash,
        word_34,
    };
    if root_text_retention::capture_early(cut, sample) {
        return;
    }
    let Ok(line) = root_text_retention::render(cut, sample) else {
        force_uart_line("[diag root-text/v1] state=invalid-record");
        return;
    };
    crate::bootstrap::log::retain_bootstrap_audit_line(line.as_str());
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LayoutSnapshot {
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

        if self.stack_top - self.stack_bottom != EXPECTED_STACK_SIZE {
            return Err(LayoutError::StackSize {
                expected: EXPECTED_STACK_SIZE,
                actual: self.stack_top - self.stack_bottom,
            });
        }

        Ok(())
    }

    fn validate_alignments(&self, alignments: &[(&'static str, usize)]) -> Result<(), LayoutError> {
        for (label, alignment) in alignments.iter().copied() {
            if alignment.count_ones() != 1 {
                return Err(LayoutError::InvalidAlignment { label, alignment });
            }

            if self.heap_start & (alignment - 1) != 0 {
                return Err(LayoutError::HeapAlignment {
                    alignment,
                    heap_start: self.heap_start,
                });
            }

            if self.stack_bottom & (alignment - 1) != 0 {
                return Err(LayoutError::StackAlignment {
                    alignment,
                    stack_bottom: self.stack_bottom,
                });
            }
        }

        Ok(())
    }

    fn fmt_report(&self) -> String<REPORT_WIDTH> {
        let mut line = String::<REPORT_WIDTH>::new();
        let _ = write!(
            line,
            "[boot:layout] text=[0x{txt_start:08x}..0x{txt_end:08x}) rodata=[0x{ro_start:08x}..0x{ro_end:08x}) data=[0x{data_start:08x}..0x{data_end:08x}) bss=[0x{bss_start:08x}..0x{bss_end:08x}) heap=[0x{heap_start:08x}..0x{heap_end:08x}) stack=[0x{stack_bottom:08x}..0x{stack_top:08x})",
            txt_start = self.text_start,
            txt_end = self.text_end,
            ro_start = self.text_end,
            ro_end = self.rodata_end,
            data_start = self.rodata_end,
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

    pub fn heap_range(&self) -> core::ops::Range<usize> {
        self.heap_start..self.heap_end
    }

    pub fn bss_range(&self) -> core::ops::Range<usize> {
        self.bss_start..self.bss_end
    }

    pub fn stack_range(&self) -> core::ops::Range<usize> {
        self.stack_bottom..self.stack_top
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayoutError {
    HeapBeforeBssEnd(usize, usize),
    HeapOverlapsStack(usize, usize),
    StackOrder(usize, usize),
    InvalidAlignment {
        label: &'static str,
        alignment: usize,
    },
    HeapAlignment {
        alignment: usize,
        heap_start: usize,
    },
    StackAlignment {
        alignment: usize,
        stack_bottom: usize,
    },
    StackSize {
        expected: usize,
        actual: usize,
    },
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
            Self::InvalidAlignment { label, alignment } => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: alignment for {label} is not a power of two (alignment=0x{alignment:08x})"
                );
            }
            Self::HeapAlignment {
                alignment,
                heap_start,
            } => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: heap_start misaligned (alignment=0x{alignment:08x} heap_start=0x{heap_start:08x})"
                );
            }
            Self::StackAlignment {
                alignment,
                stack_bottom,
            } => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: stack_bottom misaligned (alignment=0x{alignment:08x} stack_bottom=0x{stack_bottom:08x})"
                );
            }
            Self::StackSize { expected, actual } => {
                let _ = write!(
                    line,
                    "BOOT LAYOUT ERROR: stack size mismatch (expected=0x{expected:08x} actual=0x{actual:08x})"
                );
            }
        }
        line
    }
}

/// Emit a single-line layout report and halt the system if the linker-provided
/// segments overlap in an unexpected way. Safe to call before capability setup
/// or DTB parsing.
pub fn dump_and_sanity_check() -> LayoutSnapshot {
    let layout = LayoutSnapshot::from_linker();
    let report = layout.fmt_report();

    force_uart_line(report.as_str());
    log::info!("{}", report.as_str());

    let alignments = [
        ("stack", STACK_ALIGNMENT),
        ("page", 1usize << sel4_sys::seL4_PageBits),
    ];

    if let Err(err) = layout
        .validate()
        .and_then(|_| layout.validate_alignments(&alignments))
    {
        let error_line = err.render();
        force_uart_line(error_line.as_str());
        log::error!("{}", error_line.as_str());
        panic!(
            "{} layout={}",
            error_line.as_str(),
            layout.fmt_report().as_str()
        );
    }

    layout
}

#[cfg(test)]
mod tests {
    use super::{root_text_word_checksum, LayoutError, LayoutSnapshot, EXPECTED_STACK_SIZE};

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
    fn root_text_checksum_uses_fnv1a_over_little_endian_bytes() {
        // FNV-1a 32-bit reference vectors: four zero bytes, and "hell".
        assert_eq!(root_text_word_checksum(0x811c_9dc5, 0), 0x4b95_f515);
        assert_eq!(
            root_text_word_checksum(0x811c_9dc5, 0x6c6c_6568),
            0x1c71_77e6
        );
    }

    #[test]
    fn layout_validation_accepts_ordered_ranges() {
        let layout = LayoutSnapshot::new(0, 1, 2, 3, 4, 5, 8, 16, 24, 24 + EXPECTED_STACK_SIZE);
        assert_eq!(layout.validate(), Ok(()));
        assert_eq!(
            layout.validate_alignments(&[("heap", 2), ("stack", 8)]),
            Ok(())
        );
    }

    #[test]
    fn layout_validation_rejects_stack_size_mismatch() {
        let layout = LayoutSnapshot::new(0, 1, 2, 3, 4, 5, 6, 7, 8, 8 + EXPECTED_STACK_SIZE + 1);
        assert_eq!(
            layout.validate(),
            Err(LayoutError::StackSize {
                expected: EXPECTED_STACK_SIZE,
                actual: EXPECTED_STACK_SIZE + 1,
            })
        );
    }

    #[test]
    fn layout_validation_rejects_invalid_alignment() {
        let layout = LayoutSnapshot::new(0, 1, 2, 3, 4, 8, 8, 16, 16, 24);
        assert_eq!(
            layout.validate_alignments(&[("heap", 3)]),
            Err(LayoutError::InvalidAlignment {
                label: "heap",
                alignment: 3,
            })
        );
    }
}
