// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the drivers module for root-task.
// Author: Lukas Bower

//! Leaf device drivers used by the root task.

pub(crate) mod rtl8139;
#[cfg(feature = "kernel")]
pub(crate) mod virtio;
