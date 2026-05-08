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
