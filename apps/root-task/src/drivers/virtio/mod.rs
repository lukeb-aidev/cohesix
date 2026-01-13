// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the drivers/virtio module for root-task.
// Author: Lukas Bower

//! Virtio device drivers for smoltcp integration.

#[cfg(feature = "net-console")]
pub(crate) mod net;
