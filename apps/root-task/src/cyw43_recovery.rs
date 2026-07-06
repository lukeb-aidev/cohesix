// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define CYW43 owner-recovery predicates shared by root-task Wi-Fi paths and tests.
// Author: Lukas Bower

//! CYW43/SDIO owner-recovery predicates.

use pi4_driver_abi::DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK;

const CYW43_FIRMWARE_RETRY_EXHAUSTED_DETAIL: u16 = 0x5329;

pub(crate) const fn firmware_resume_forces_byte_mode(op: u16, detail: u16) -> bool {
    op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
        && detail != CYW43_FIRMWARE_RETRY_EXHAUSTED_DETAIL
        && fault_detail_allows_sdio_owner_recovery(detail)
}

pub(crate) const fn fault_detail_allows_sdio_owner_recovery(detail: u16) -> bool {
    matches!(
        detail,
        0x5101
            | 0x5102
            | 0x5103
            | 0x5104
            | 0x5310
            | 0x531a
            | 0x531b
            | 0x531c
            | 0x531d
            | 0x531e
            | 0x531f
            | 0x5321
            | 0x5323
            | 0x5329
            | 0x532a
            | 0x532b
            | 0x532c
            | 0x532d
    )
}

pub(crate) const fn fault_detail_allows_same_command_retry(detail: u16) -> bool {
    matches!(detail, 0x5103 | 0x532b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi4_driver_abi::{
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK, DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
    };

    #[test]
    fn firmware_resume_retries_retry_exhaustion_on_primary_lane() {
        assert!(!firmware_resume_forces_byte_mode(
            DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            0x5329
        ));
        assert!(firmware_resume_forces_byte_mode(
            DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            0x5103
        ));
        assert!(!firmware_resume_forces_byte_mode(
            DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
            0x5329
        ));
        assert!(!firmware_resume_forces_byte_mode(
            DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            0x5302
        ));
    }

    #[test]
    fn transport_recovery_is_limited_to_owner_backplane_faults() {
        assert!(fault_detail_allows_sdio_owner_recovery(0x5323));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5321));
        assert!(fault_detail_allows_sdio_owner_recovery(0x531a));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5101));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5102));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5103));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5104));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5329));
        assert!(fault_detail_allows_sdio_owner_recovery(0x532a));
        assert!(fault_detail_allows_sdio_owner_recovery(0x532b));
        assert!(fault_detail_allows_sdio_owner_recovery(0x532c));
        assert!(fault_detail_allows_sdio_owner_recovery(0x532d));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x532e));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x5302));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x5306));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x53ff));
        assert!(fault_detail_allows_same_command_retry(0x5103));
        assert!(fault_detail_allows_same_command_retry(0x532b));
        assert!(!fault_detail_allows_same_command_retry(0x532a));
        assert!(!fault_detail_allows_same_command_retry(0x5102));
    }
}
