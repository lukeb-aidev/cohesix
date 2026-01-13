// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the boot module for root-task.
// Author: Lukas Bower
//! Root-task bootstrap helpers for kernel endpoint initialisation.

/// Helpers for parsing bootinfo extra records emitted by the kernel.
pub mod bi_extra;
/// Kernel endpoint bootstrap scaffolding.
pub mod ep;
pub mod flags;
pub mod tcb;
pub mod uart_pl011;
