// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Own platform UART physical addresses for kernel console probing.
// Author: Lukas Bower

//! Platform UART address descriptors owned by the HAL boundary.

/// QEMU `virt` PL011 UART physical base.
pub const QEMU_PL011_PADDR: usize = 0x0900_0000;

/// Raspberry Pi 4 mini-UART physical base.
pub const PI4_MINI_UART_PADDR: usize = 0xFE21_5000;

/// Raspberry Pi 4 PL011 UART0 physical base.
pub const PI4_PL011_PADDR: usize = 0xFE20_1000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uart_addresses_match_qemu_and_pi4_platform_windows() {
        assert_eq!(QEMU_PL011_PADDR, 0x0900_0000);
        assert_eq!(PI4_MINI_UART_PADDR, 0xFE21_5000);
        assert_eq!(PI4_PL011_PADDR, 0xFE20_1000);
        assert_ne!(PI4_MINI_UART_PADDR, PI4_PL011_PADDR);
        assert_eq!(QEMU_PL011_PADDR & 0xff00_0000, 0x0900_0000);
        assert_eq!(PI4_MINI_UART_PADDR & 0xffff_0000, 0xfe21_0000);
        assert_eq!(PI4_PL011_PADDR & 0xffff_0000, 0xfe20_0000);
    }
}
