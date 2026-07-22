// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define CYW43 owner-recovery predicates shared by root-task Wi-Fi paths and tests.
// Author: Lukas Bower

//! CYW43/SDIO owner-recovery predicates.

use pi4_driver_abi::DRIVER_RUNTIME_CYW43_OP_RELEASE;

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
            | 0x5322
            | 0x5323
            | 0x5329
            | 0x532a
            | 0x532b
            | 0x532c
            | 0x532d
            | 0x532f
            | 0x5330
            | 0x5331
            | 0x5332
            | 0x5333
            | 0x5334
            | 0x5335
            | 0x5336
            | 0x5337
            | 0x5338
    )
}

pub(crate) const fn fault_detail_allows_same_command_retry(detail: u16) -> bool {
    detail == 0x5103
}

pub(crate) const fn firmware_release_fault_requires_engine_recovery(op: u16, detail: u16) -> bool {
    op == DRIVER_RUNTIME_CYW43_OP_RELEASE && fault_detail_allows_sdio_owner_recovery(detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi4_driver_abi::DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK;

    #[test]
    fn transport_recovery_is_limited_to_owner_backplane_faults() {
        assert!(fault_detail_allows_sdio_owner_recovery(0x5323));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5321));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5322));
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
        assert!(fault_detail_allows_sdio_owner_recovery(0x532f));
        assert!(fault_detail_allows_sdio_owner_recovery(0x5330));
        for detail in [
            0x5331, 0x5332, 0x5333, 0x5334, 0x5335, 0x5336, 0x5337, 0x5338,
        ] {
            assert!(fault_detail_allows_sdio_owner_recovery(detail));
            assert!(!fault_detail_allows_same_command_retry(detail));
        }
        assert!(!fault_detail_allows_sdio_owner_recovery(0x532e));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x5302));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x5306));
        assert!(!fault_detail_allows_sdio_owner_recovery(0x53ff));
        assert!(fault_detail_allows_same_command_retry(0x5103));
        assert!(!fault_detail_allows_same_command_retry(0x532b));
        assert!(!fault_detail_allows_same_command_retry(0x532a));
        assert!(!fault_detail_allows_same_command_retry(0x5102));
    }

    #[test]
    fn release_faults_route_to_engine_recovery_by_operation() {
        for detail in [0x5101, 0x531a, 0x5321, 0x5322, 0x532a, 0x532f, 0x5330] {
            assert!(firmware_release_fault_requires_engine_recovery(
                DRIVER_RUNTIME_CYW43_OP_RELEASE,
                detail
            ));
        }
        assert!(!firmware_release_fault_requires_engine_recovery(
            DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            0x532a
        ));
        assert!(!firmware_release_fault_requires_engine_recovery(
            DRIVER_RUNTIME_CYW43_OP_RELEASE,
            0x532e
        ));
    }
}
