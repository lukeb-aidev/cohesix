// Author: Lukas Bower
// Purpose: Admit bounded console reads only within immutable root executable code.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! Emergency diagnostics cannot read MMIO, mutable memory or embedded secrets.

use core::ops::Range;

const MAX_READ_BYTES: usize = 256;

/// Deterministic refusal before any requested address is dereferenced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticReadError {
    Disabled,
    Length,
    Unclassified,
}

#[cfg(feature = "kernel")]
impl DiagnosticReadError {
    pub(crate) const fn terminal(self) -> &'static str {
        match self {
            Self::Disabled => "ERR EPERM memory-diagnostics-disabled",
            Self::Length => "ERR ELIMIT memory-diagnostics-length",
            Self::Unclassified => "ERR EPERM memory-diagnostics-unclassified",
        }
    }
}

/// Classify one read against the selected policy and root image's RX code span.
///
/// The linker places `.text` before `__driver_task_text_start` in the initial
/// read/execute PT_LOAD. That span remains mapped for the root lifetime. It
/// excludes driver/Worker archives, rodata credentials, writable state and MMIO.
#[cfg(feature = "kernel")]
pub(crate) fn console_read(
    address: usize,
    length: usize,
) -> Result<Range<usize>, DiagnosticReadError> {
    #[cfg(all(feature = "kernel", target_os = "none"))]
    let code = {
        extern "C" {
            static __text_start: u8;
            static __driver_task_text_start: u8;
        }
        core::ptr::addr_of!(__text_start) as usize
            ..core::ptr::addr_of!(__driver_task_text_start) as usize
    };
    #[cfg(not(all(feature = "kernel", target_os = "none")))]
    let code = 0..0;

    classify(
        crate::generated::AUTHORITY_POLICY.debug_memory,
        cfg!(feature = "release-qemu") || cfg!(feature = "release-pi4"),
        code,
        address,
        length,
    )
}

fn classify(
    enabled: bool,
    production: bool,
    code: Range<usize>,
    address: usize,
    length: usize,
) -> Result<Range<usize>, DiagnosticReadError> {
    if !enabled || production {
        return Err(DiagnosticReadError::Disabled);
    }
    if length == 0 || length > MAX_READ_BYTES {
        return Err(DiagnosticReadError::Length);
    }
    let end = address
        .checked_add(length)
        .ok_or(DiagnosticReadError::Unclassified)?;
    if code.start >= code.end || address < code.start || end > code.end {
        return Err(DiagnosticReadError::Unclassified);
    }
    Ok(address..end)
}

#[cfg(test)]
mod tests {
    use super::{classify, DiagnosticReadError};

    #[test]
    fn production_and_unselected_diagnostics_deny_before_address_checks() {
        for (enabled, production) in [(false, false), (false, true), (true, true)] {
            assert_eq!(
                classify(enabled, production, 0x1000..0x2000, usize::MAX, 0),
                Err(DiagnosticReadError::Disabled)
            );
        }
    }

    #[test]
    fn bringup_reads_include_exact_code_boundaries() {
        assert_eq!(
            classify(true, false, 0x1000..0x2000, 0x1000, 256),
            Ok(0x1000..0x1100)
        );
        assert_eq!(
            classify(true, false, 0x1000..0x2000, 0x1fff, 1),
            Ok(0x1fff..0x2000)
        );
    }

    #[test]
    fn requested_lengths_are_refused_without_silent_truncation() {
        for length in [0, 257, usize::MAX] {
            assert_eq!(
                classify(true, false, 0x1000..0x2000, 0x1000, length),
                Err(DiagnosticReadError::Length)
            );
        }
    }

    #[test]
    fn unclassified_cross_boundary_and_overflow_reads_are_refused() {
        for (code, address, length) in [
            (0x1000..0x2000, 0xfff, 2),
            (0x1000..0x2000, 0x2000, 1),
            (0x1000..0x2000, 0x1fff, 2),
            (0x1000..0x2000, usize::MAX, 1),
            (0..0, 0, 1),
        ] {
            assert_eq!(
                classify(true, false, code, address, length),
                Err(DiagnosticReadError::Unclassified)
            );
        }
    }
}
