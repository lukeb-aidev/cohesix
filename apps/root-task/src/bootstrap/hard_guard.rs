// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/hard_guard module for root-task.
// Author: Lukas Bower

#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::Write;

use heapless::String;
use sel4_sys::seL4_Error;

use crate::bootstrap::log::force_uart_line;

#[derive(Copy, Clone, Debug)]
pub enum HardGuardViolation {
    BootInfoCanaryDiverged,
    BootInfoSnapshotOutOfRange,
    CSpaceWindowInvalid,
    EPInvalidOrNotInEmptyWindow,
    EPIdentifyInvalid { ident: u32 },
    IPCBufferMissing,
    IPCBufferNotAligned,
    IPCBufferSetRejected { err: seL4_Error },
}

#[inline(always)]
pub fn hard_guard_fail(tag: &str, v: HardGuardViolation) -> ! {
    let mut line = String::<96>::new();
    let _ = write!(line, "[HARD_GUARD] tag={} v=", tag);

    let _ = match v {
        HardGuardViolation::BootInfoCanaryDiverged => line.push_str("BootInfoCanaryDiverged"),
        HardGuardViolation::BootInfoSnapshotOutOfRange => {
            line.push_str("BootInfoSnapshotOutOfRange")
        }
        HardGuardViolation::CSpaceWindowInvalid => line.push_str("CSpaceWindowInvalid"),
        HardGuardViolation::EPInvalidOrNotInEmptyWindow => {
            line.push_str("EPInvalidOrNotInEmptyWindow")
        }
        HardGuardViolation::EPIdentifyInvalid { ident } => {
            let _ = write!(line, "EPIdentifyInvalid{{ident={}}}", ident);
            Ok(())
        }
        HardGuardViolation::IPCBufferMissing => line.push_str("IPCBufferMissing"),
        HardGuardViolation::IPCBufferNotAligned => line.push_str("IPCBufferNotAligned"),
        HardGuardViolation::IPCBufferSetRejected { err } => {
            let _ = write!(line, "IPCBufferSetRejected{{err={}}}", err as isize);
            Ok(())
        }
    };

    force_uart_line(line.as_str());

    #[cfg(feature = "strict-bootstrap")]
    panic!("{}", line);

    #[cfg(not(feature = "strict-bootstrap"))]
    loop {
        crate::sel4::yield_now();
    }
}
