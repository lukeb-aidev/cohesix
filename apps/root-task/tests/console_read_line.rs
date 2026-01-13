// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task console_read_line.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::mem::size_of;

use root_task::console::Console;
use root_task::serial::pl011::Pl011;

#[test]
fn build_only_console() {
    let _ = size_of::<Console>();
    let _ = size_of::<Pl011>();
    assert!(true);
}
