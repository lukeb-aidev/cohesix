// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the panic module for root-task.
// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt::Write;
use core::panic::PanicInfo;

use heapless::String;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut line = String::<192>::new();
    let _ = write!(&mut line, "[PANIC] {}", info);
    crate::bootstrap::log::force_uart_line(line.as_str());
    crate::kernel::panic_handler(info)
}
