// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task cnode_depth.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::sel4::init_cnode_depth;
use sel4_sys::seL4_BootInfo;

#[test]
fn cnode_depth_matches_bootinfo_bits() {
    let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
    bootinfo.initThreadCNodeSizeBits = 13;
    assert_eq!(
        init_cnode_depth(&bootinfo),
        bootinfo.initThreadCNodeSizeBits
    );
}
